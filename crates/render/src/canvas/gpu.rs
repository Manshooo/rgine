use std::collections::HashMap;
use std::ops::Range;

use super::{Canvas, Vec2, Viewport};
use crate::RenderError;
use crate::profile::RenderSample;
use crate::resources::{
    BufferDescriptor, BufferHandle, BufferKind, ResourceError, ResourceTable, SampledBinding,
    SampledGroupHandle, SamplerDescriptor, TextureDescriptor, checked_gpu,
};

const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
const STRIDE: usize = 32;

struct Draw {
    vertices: Range<u32>,
    scissor: [u32; 4],
    texture: SampledGroupHandle,
}

#[derive(Default)]
pub(crate) struct CanvasRenderer {
    white: Option<SampledGroupHandle>,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    buffer: Option<(BufferHandle, u64)>,
    bytes: Vec<u8>,
    draws: Vec<Draw>,
}

impl CanvasRenderer {
    pub(crate) fn prepare(
        &mut self,
        table: &mut ResourceTable,
        canvas: &Canvas,
        viewport: Viewport,
        format: wgpu::TextureFormat,
    ) -> Result<bool, RenderError> {
        if canvas.batches.is_empty() {
            return Ok(false);
        }
        let white = match self.white {
            Some(handle) => handle,
            None => {
                let handle = white_group(table)?;
                self.white = Some(handle);
                handle
            }
        };
        let layout = table.sampled_group(white)?.layout.clone();
        self.bytes.clear();
        self.draws.clear();
        let logical = viewport.logical_size();
        for batch in &canvas.batches {
            let Some(scissor) = viewport.scissor(batch.clip) else {
                continue;
            };
            let texture = batch.texture.unwrap_or(white);
            if table.sampled_group(texture)?.layout != layout {
                return Err(RenderError(
                    "canvas texture requires a filterable 2D color view and a filtering sampler"
                        .into(),
                ));
            }
            let start = self.vertex_count()?;
            for vertex in &canvas.vertices[batch.vertices.clone()] {
                let position =
                    vertex.position / logical * Vec2::new(2.0, -2.0) + Vec2::new(-1.0, 1.0);
                self.bytes.extend(
                    position
                        .to_array()
                        .into_iter()
                        .chain(vertex.uv.to_array())
                        .chain(vertex.color)
                        .flat_map(f32::to_le_bytes),
                );
            }
            let end = self.vertex_count()?;
            if let Some(previous) = self.draws.last_mut()
                && previous.scissor == scissor
                && previous.texture == texture
            {
                previous.vertices.end = end;
            } else {
                self.draws.push(Draw {
                    vertices: start..end,
                    scissor,
                    texture,
                });
            }
        }
        if self.draws.is_empty() {
            return Ok(false);
        }
        self.upload(table)?;
        if let std::collections::hash_map::Entry::Vacant(entry) = self.pipelines.entry(format) {
            entry.insert(create_pipeline(&table.device, &layout, format)?);
        }
        Ok(true)
    }

    pub(crate) fn pipeline_creations(&self) -> usize {
        self.pipelines.len()
    }

    pub(crate) fn encode(
        &self,
        table: &ResourceTable,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        profile: Option<&mut RenderSample>,
    ) -> Result<usize, RenderError> {
        let buffer = self.buffer.expect("prepared canvas buffer").0;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen-space canvas"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipelines[&format]);
        pass.set_vertex_buffer(
            0,
            table.gpu_buffer(buffer)?.slice(..self.bytes.len() as u64),
        );
        let mut work = profile.map(|profile| profile.pass("screen-space canvas"));
        if let Some(work) = &mut work {
            work.pipeline_changes = 1;
        }
        let mut previous_texture = None;
        for draw in &self.draws {
            let [x, y, width, height] = draw.scissor;
            pass.set_scissor_rect(x, y, width, height);
            pass.set_bind_group(0, &table.sampled_group(draw.texture)?.raw, &[]);
            pass.draw(draw.vertices.clone(), 0..1);
            if let Some(work) = &mut work {
                work.binding(previous_texture.replace(draw.texture) != Some(draw.texture));
                work.draw(draw.vertices.len() as u32 / 3);
            }
        }
        Ok(self.draws.len())
    }

    fn vertex_count(&self) -> Result<u32, RenderError> {
        u32::try_from(self.bytes.len() / STRIDE)
            .map_err(|_| RenderError("canvas vertex count exceeds the draw limit".into()))
    }

    fn upload(&mut self, table: &mut ResourceTable) -> Result<BufferHandle, ResourceError> {
        let size = self.bytes.len() as u64;
        let descriptor = BufferDescriptor {
            kind: BufferKind::Vertex,
            size: size
                .next_power_of_two()
                .min(table.device.limits().max_buffer_size)
                .max(size),
        };
        let handle = match self.buffer {
            Some((handle, capacity)) if capacity >= size => handle,
            Some((handle, _)) => {
                table.replace_buffer(handle, descriptor)?;
                self.buffer = Some((handle, descriptor.size));
                handle
            }
            None => {
                let handle = table.create_buffer(descriptor)?;
                self.buffer = Some((handle, descriptor.size));
                handle
            }
        };
        table.write_buffer(handle, 0, &self.bytes)?;
        Ok(handle)
    }
}

fn white_group(table: &mut ResourceTable) -> Result<SampledGroupHandle, ResourceError> {
    let texture = table.create_texture(TextureDescriptor::sampled_rgba8([1, 1]))?;
    let sampler = match table.create_sampler(SamplerDescriptor::NEAREST_CLAMP) {
        Ok(sampler) => sampler,
        Err(error) => {
            table.release_texture(texture)?;
            return Err(error);
        }
    };
    let group = table
        .write_texture(texture, 0, 0, &[255; 4])
        .and_then(|()| table.create_sampled_group(SampledBinding { texture, sampler }));
    if group.is_err() {
        table.release_sampler(sampler)?;
        table.release_texture(texture)?;
    }
    group
}

fn create_pipeline(
    device: &wgpu::Device,
    texture: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, ResourceError> {
    let label = Some("screen-space canvas");
    checked_gpu(device, || {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label,
            source: wgpu::ShaderSource::Wgsl(include_str!("canvas.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label,
            bind_group_layouts: &[Some(texture)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label,
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: STRIDE as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    })
}
