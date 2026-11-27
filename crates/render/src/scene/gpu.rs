use std::collections::{HashMap, hash_map::Entry};
use std::num::NonZeroU64;
use std::time::Instant;

use engine_asset::ktx2::{ColorSpace, Texture};
use engine_core::glam::{Mat4, Vec3};

use super::{
    DirectionalLight, HemisphereLight, MaterialTexture, Mesh, MeshKey, MeshRegistry, PbrMaterial,
    Scene, SceneObject,
};
use crate::RenderError;
use crate::graph::{CompiledGraph, RenderGraph};
use crate::profile::{PassWork, RenderSample};
use crate::resources::{
    BINDLESS_SAMPLERS, BINDLESS_TEXTURES, BindingMode, BufferDescriptor, BufferHandle, BufferKind,
    ResourceError, ResourceTable, SampledBinding, SampledGroupHandle, SamplerDescriptor,
    SamplerFilter, SamplerHandle, TextureDescriptor, TextureFormat, TextureHandle, UniformBinding,
    UniformGroupHandle, checked_gpu,
};
use crate::shaders::{SceneShaderSources, ShaderVariantKey};

mod shadows;
mod skybox;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const UNIFORM_SIZE: NonZeroU64 = NonZeroU64::new(336).unwrap();
const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

pub(crate) const SCENE_COLOR_FORMATS: &[wgpu::TextureFormat] = &[
    wgpu::TextureFormat::Bgra8UnormSrgb,
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8UnormSrgb,
    wgpu::TextureFormat::Rgba8Unorm,
    wgpu::TextureFormat::Rgb10a2Unorm,
    wgpu::TextureFormat::Rgba16Float,
];

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

    fn for_config(
        format: wgpu::TextureFormat,
        binding_mode: BindingMode,
        skybox: bool,
    ) -> impl Iterator<Item = Self> {
        [Self::new(format, binding_mode), Self::DIRECTIONAL_SHADOW]
            .into_iter()
            .chain(skybox.then(|| Self::skybox(format)))
    }
}

#[derive(Default)]
struct ScenePipelines {
    entries: HashMap<ScenePipelineKey, wgpu::RenderPipeline>,
    created: usize,
    shadow_layout: Option<wgpu::BindGroupLayout>,
    sources: SceneShaderSources,
}

impl ScenePipelines {
    fn prepare(
        &mut self,
        table: &mut ResourceTable,
        format: wgpu::TextureFormat,
        skybox: bool,
    ) -> Result<(), RenderError> {
        let mode = table.binding_mode();
        let keys = || ScenePipelineKey::for_config(format, mode, skybox);
        if keys().all(|key| self.entries.contains_key(&key)) {
            return Ok(());
        }
        let start = Instant::now();
        let before = self.entries.len();
        if self.shadow_layout.is_none() {
            self.shadow_layout = Some(shadows::lookup_layout(&table.device)?);
        }
        let shadow = self.shadow_layout.as_ref().expect("shadow layout prepared");
        for key in keys() {
            let Entry::Vacant(entry) = self.entries.entry(key) else {
                continue;
            };
            let pipeline = build_scene_pipeline(table, key, shadow, &self.sources)?;
            entry.insert(pipeline);
            self.created += 1;
            tracing::debug!(?key, "scene pipeline created");
        }
        tracing::info!(
            ?format,
            binding_mode = ?mode,
            skybox,
            pipeline_creations = self.entries.len() - before,
            elapsed_ms = start.elapsed().as_secs_f64() * 1000.0,
            "scene pipelines prepared"
        );
        Ok(())
    }

    #[cfg(feature = "shader-reload")]
    fn reload(
        &mut self,
        table: &mut ResourceTable,
        sources: &SceneShaderSources,
    ) -> Result<bool, RenderError> {
        let mut replacements = Vec::new();
        let shadow = self
            .shadow_layout
            .as_ref()
            .expect("scene pipelines prepared before reload");
        for &key in self.entries.keys() {
            if key.shader.changed(&self.sources, sources) {
                let pipeline =
                    build_scene_pipeline(table, key, shadow, sources).map_err(|error| {
                        let files = sources
                            .changed_files(&self.sources)
                            .collect::<Vec<_>>()
                            .join(", ");
                        RenderError(format!("{files}: {error}"))
                    })?;
                replacements.push((key, pipeline));
            }
        }
        let changed = !replacements.is_empty();
        self.created += replacements.len();
        self.entries.extend(replacements);
        self.sources = sources.clone();
        Ok(changed)
    }
}

fn build_scene_pipeline(
    table: &mut ResourceTable,
    key: ScenePipelineKey,
    shadow: &wgpu::BindGroupLayout,
    sources: &SceneShaderSources,
) -> Result<wgpu::RenderPipeline, RenderError> {
    let device = table.device.clone();
    let uniforms = table.uniform_layout(1)?.clone();
    if let ShaderVariantKey::Primitive(mode) = key.shader {
        let sampled = match mode {
            BindingMode::Classic => table.sampled_layout(
                TextureDescriptor::sampled_rgba8([1, 1]).view,
                wgpu::SamplerBindingType::Filtering,
            )?,
            BindingMode::Bindless => table.bindless_layout()?,
        };
        create_pipeline(
            &device,
            key,
            &[Some(&uniforms), Some(sampled), Some(shadow)],
            sources,
        )
    } else {
        create_pipeline(&device, key, &[Some(&uniforms)], sources)
    }
}

#[derive(Default)]
pub(crate) struct SceneRenderer {
    pub(crate) light: DirectionalLight,
    pub(crate) hemisphere_light: HemisphereLight,
    pub(crate) skybox_enabled: bool,
    skybox: Option<skybox::Skybox>,
    pipelines: ScenePipelines,
    target: Option<SceneTargets>,
    registry: Option<u64>,
    meshes: HashMap<MeshKey, GpuMesh>,
    materials: HashMap<MaterialTexture, (MaterialBinding, Option<TextureHandle>)>,
    imported_sampler: Option<SamplerHandle>,
    uniforms: Option<ObjectUniforms>,
}

impl SceneRenderer {
    #[cfg(feature = "shader-reload")]
    pub(crate) fn reload_shaders(
        &mut self,
        table: &mut ResourceTable,
        sources: &SceneShaderSources,
    ) -> Result<bool, RenderError> {
        self.pipelines.reload(table, sources)
    }

    pub(crate) fn pipeline_keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self
            .pipelines
            .entries
            .keys()
            .map(|key| format!("{key:?}"))
            .collect();
        keys.sort_unstable();
        keys
    }

    pub(crate) fn prepare_pipelines(
        &mut self,
        table: &mut ResourceTable,
        format: wgpu::TextureFormat,
    ) -> Result<(), RenderError> {
        self.pipelines.prepare(table, format, self.skybox_enabled)
    }

    pub(crate) fn pipeline_creations(&self) -> usize {
        self.pipelines.created
    }

    pub(crate) fn prepare(
        &mut self,
        table: &mut ResourceTable,
        meshes: &MeshRegistry,
        scene: &Scene,
        size: [u32; 2],
    ) -> Result<(), RenderError> {
        if self.registry != Some(meshes.id) {
            for (_, mesh) in self.meshes.drain() {
                table.release_buffer(mesh.vertices)?;
                table.release_buffer(mesh.indices)?;
            }
            for (surface, material) in std::mem::take(&mut self.materials) {
                if let Some(texture) = material.1 {
                    table.release_texture(texture)?;
                } else {
                    self.materials.insert(surface, material);
                }
            }
            self.registry = Some(meshes.id);
        }
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.size != size || target.skybox != self.skybox_enabled)
        {
            let shadow_layout = self
                .pipelines
                .shadow_layout
                .as_ref()
                .expect("pipelines prepared");
            self.target = Some(SceneTargets::new(
                table,
                size,
                self.skybox_enabled,
                shadow_layout,
            )?);
        }
        let Some(camera) = scene.camera else {
            return Ok(());
        };
        let clip = camera
            .clip_from_world(size)
            .ok_or_else(|| RenderError("invalid perspective camera or drawable size".into()))?;
        if self.skybox_enabled {
            if self.skybox.is_none() {
                self.skybox = Some(skybox::Skybox::new(table)?);
            }
            self.skybox
                .as_mut()
                .expect("skybox allocated")
                .prepare(table, camera, size)?;
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
            let Entry::Vacant(entry) = self.meshes.entry(object.mesh) else {
                continue;
            };
            let mesh = meshes.get(object.mesh).ok_or_else(|| {
                RenderError(format!(
                    "mesh {:?} is missing from the scene registry",
                    object.mesh
                ))
            })?;
            let binding = match self.materials.entry(mesh.surface) {
                Entry::Occupied(entry) => entry.get().0,
                Entry::Vacant(entry) => {
                    entry
                        .insert(match mesh.surface {
                            MaterialTexture::Solid => {
                                (MaterialBinding::builtin(table, false)?, None)
                            }
                            MaterialTexture::Checkerboard => {
                                (MaterialBinding::builtin(table, true)?, None)
                            }
                            MaterialTexture::Imported { index, .. } => {
                                let texture = &meshes.textures[index];
                                if self.imported_sampler.is_none() {
                                    self.imported_sampler =
                                        Some(table.create_sampler(SamplerDescriptor {
                                            filter: SamplerFilter::Linear,
                                            ..SamplerDescriptor::NEAREST_CLAMP
                                        })?);
                                }
                                MaterialBinding::imported(
                                    table,
                                    texture,
                                    self.imported_sampler.expect("imported texture sampler"),
                                )
                                .map(|(binding, texture)| (binding, Some(texture)))
                                .map_err(|e| {
                                    RenderError(format!("{}: {e}", texture.source().display()))
                                })?
                            }
                        })
                        .0
                }
            };
            entry.insert(GpuMesh::upload(table, mesh, binding)?);
        }
        let mode = table.binding_mode();
        if mode == BindingMode::Bindless {
            table.prepare_bindless()?;
        }
        if self.uniforms.is_none() {
            self.uniforms = Some(ObjectUniforms::new(table, scene.objects.len())?);
        }
        let meshes = &self.meshes;
        let frame = SceneUniforms {
            clip,
            eye,
            light: self.light,
            hemisphere_light: self.hemisphere_light,
            shadow_clip: shadows::clip_from_world(self.light, &scene.objects, &self.meshes),
        };
        self.uniforms
            .as_mut()
            .expect("object buffers allocated")
            .upload(table, frame, &scene.objects, |object| {
                let mesh = &meshes[&object.mesh];
                (mesh.material, mesh.binding.indices())
            })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the optional profile belongs to the same render attempt"
    )]
    pub(crate) fn encode(
        &self,
        table: &ResourceTable,
        scene: &Scene,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        clear: wgpu::Color,
        mut profile: Option<&mut RenderSample>,
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
                            profile
                                .as_mut()
                                .map(|profile| profile.pass(ScenePass::DirectionalShadow.name())),
                        )?;
                        if let Some(profile) = &mut profile {
                            let work = profile.pass(ScenePass::Skybox.name());
                            work.pipeline_changes = 1;
                            work.binding(true);
                            work.draw(1);
                        }
                    }
                    return Ok(());
                }
                ScenePass::Skybox => {
                    if draw_skybox {
                        self.skybox.as_ref().expect("skybox prepared").encode(
                            table,
                            encoder,
                            &self.pipelines.entries[&ScenePipelineKey::skybox(format)],
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
            let mut work = profile
                .as_mut()
                .map(|profile| profile.pass(ScenePass::Opaque.name()));
            if has_draws {
                let uniforms = self.uniforms.as_ref().expect("object bindings prepared");
                let key = ScenePipelineKey::new(format, table.binding_mode());
                let pipeline = &self.pipelines.entries[&key];
                render.set_pipeline(pipeline);
                render.set_bind_group(2, &target.shadow.group, &[]);
                if table.binding_mode() == BindingMode::Bindless {
                    render.set_bind_group(1, table.bindless_group()?, &[]);
                }
                if let Some(work) = &mut work {
                    work.pipeline_changes = 1;
                    work.binding(true);
                    if table.binding_mode() == BindingMode::Bindless {
                        work.binding(true);
                    }
                }
                let mut previous_material = None;
                for (object, group) in scene.objects.iter().zip(&uniforms.groups) {
                    let mesh = &self.meshes[&object.mesh];
                    render.set_bind_group(0, table.gpu_uniform_group(*group)?, &[]);
                    if let MaterialBinding::Classic(group) = mesh.binding {
                        render.set_bind_group(1, &table.sampled_group(group)?.raw, &[]);
                    }
                    mesh.draw(table, &mut render)?;
                    if let Some(work) = &mut work {
                        work.binding(true);
                        if let MaterialBinding::Classic(group) = mesh.binding {
                            work.binding(previous_material.replace(group) != Some(group));
                        }
                        work.draw(mesh.index_count / 3);
                    }
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
        mut work: Option<&mut PassWork>,
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
        render.set_pipeline(&self.pipelines.entries[&ScenePipelineKey::DIRECTIONAL_SHADOW]);
        if let Some(work) = &mut work {
            work.pipeline_changes = 1;
        }
        let uniforms = self.uniforms.as_ref().expect("object bindings prepared");
        for (object, group) in scene.objects.iter().zip(&uniforms.groups) {
            render.set_bind_group(0, table.gpu_uniform_group(*group)?, &[]);
            let mesh = &self.meshes[&object.mesh];
            mesh.draw(table, &mut render)?;
            if let Some(work) = &mut work {
                work.binding(true);
                work.draw(mesh.index_count / 3);
            }
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

impl ScenePass {
    fn name(self) -> &'static str {
        match self {
            Self::DirectionalShadow => "directional shadow",
            Self::Skybox => "skybox",
            Self::Opaque => "opaque scene",
        }
    }
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
    fn new(
        table: &ResourceTable,
        size: [u32; 2],
        skybox: bool,
        shadow_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, RenderError> {
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
            graph.add_pass(
                ScenePass::Skybox.name(),
                &[],
                &[background],
                ScenePass::Skybox,
            );
            opaque_reads.push(background);
        }
        graph.add_pass(
            ScenePass::Opaque.name(),
            &opaque_reads,
            &[color, depth],
            ScenePass::Opaque,
        );
        graph.add_pass(
            ScenePass::DirectionalShadow.name(),
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
        let shadow = shadows::ShadowTarget::new(&table.device, shadow_layout, view(shadow))?;
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
    binding: MaterialBinding,
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

    fn upload(
        table: &mut ResourceTable,
        mesh: &Mesh,
        binding: MaterialBinding,
    ) -> Result<Self, RenderError> {
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
                binding,
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
    fn builtin(table: &mut ResourceTable, checkerboard: bool) -> Result<Self, ResourceError> {
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
        let material = table
            .write_texture(texture, 0, 0, texels)
            .and_then(|()| Self::bind(table, SampledBinding { texture, sampler }));
        if material.is_err() {
            table.release_sampler(sampler)?;
            table.release_texture(texture)?;
        }
        material
    }

    fn imported(
        table: &mut ResourceTable,
        asset: &Texture,
        sampler: SamplerHandle,
    ) -> Result<(Self, TextureHandle), ResourceError> {
        let mut descriptor = TextureDescriptor::sampled_rgba8(asset.size());
        descriptor.format = match asset.color_space() {
            ColorSpace::Linear => TextureFormat::Rgba8Unorm,
            ColorSpace::Srgb => TextureFormat::Rgba8UnormSrgb,
        };
        let levels = asset.levels();
        descriptor.mip_levels = levels.len() as u32;
        descriptor.view.mip_level_count = descriptor.mip_levels;
        let texture = table.create_texture(descriptor)?;
        let material = levels
            .enumerate()
            .try_for_each(|(mip, texels)| table.write_texture(texture, mip as u32, 0, texels))
            .and_then(|()| Self::bind(table, SampledBinding { texture, sampler }));
        if material.is_err() {
            table.release_texture(texture)?;
        }
        material.map(|binding| (binding, texture))
    }

    fn bind(table: &mut ResourceTable, binding: SampledBinding) -> Result<Self, ResourceError> {
        match table.binding_mode() {
            BindingMode::Classic => table.create_sampled_group(binding).map(Self::Classic),
            BindingMode::Bindless => table.bindless_indices(binding).map(Self::Bindless),
        }
    }

    fn indices(self) -> [u32; 2] {
        match self {
            Self::Classic(_) => [0; 2],
            Self::Bindless(indices) => indices,
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
    hemisphere_light: HemisphereLight,
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
            for (bytes, value) in slot.as_chunks_mut::<4>().0.iter_mut().zip(values) {
                *bytes = value.to_le_bytes();
            }
            slot[228..232].copy_from_slice(&texture.to_le_bytes());
            slot[232..236].copy_from_slice(&sampler.to_le_bytes());
            let lighting = frame
                .shadow_clip
                .to_cols_array()
                .into_iter()
                .chain(frame.hemisphere_light.upper.extend(0.0).to_array())
                .chain(frame.hemisphere_light.lower.extend(0.0).to_array());
            for (bytes, value) in slot[240..UNIFORM_SIZE.get() as usize]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(lighting)
            {
                *bytes = value.to_le_bytes();
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
    sources: &SceneShaderSources,
) -> Result<wgpu::RenderPipeline, RenderError> {
    let source = key
        .shader
        .wgsl(sources)
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
    let shader = checked_gpu(device, || {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        })
    })
    .map_err(|error| RenderError(format!("invalid scene shader {label}: {error}")))?;
    checked_gpu(device, || {
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
    })
    .map_err(|error| RenderError(format!("invalid scene pipeline {key:?}: {error}")))
}

#[cfg(test)]
mod tests;
