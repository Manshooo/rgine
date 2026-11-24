use std::num::NonZeroU64;

use engine_core::glam::{Mat3, Mat4};

use super::ScenePipelineKey;
use crate::RenderError;
use crate::resources::{
    BufferDescriptor, BufferHandle, BufferKind, ResourceTable, UniformBinding, UniformGroupHandle,
};
use crate::scene::SceneCamera;

pub(super) const UNIFORM_SIZE: NonZeroU64 = NonZeroU64::new(64).unwrap();

pub(super) struct Skybox {
    pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    uniform: BufferHandle,
    group: UniformGroupHandle,
}

impl Skybox {
    pub(super) fn new(
        table: &mut ResourceTable,
        format: wgpu::TextureFormat,
    ) -> Result<Self, RenderError> {
        let pipeline = create_pipeline(table, format)?;
        let uniform = table.create_buffer(BufferDescriptor {
            kind: BufferKind::Uniform,
            size: UNIFORM_SIZE.get(),
        })?;
        let group = match table.create_uniform_group(&[UniformBinding {
            buffer: uniform,
            offset: 0,
            size: UNIFORM_SIZE,
        }]) {
            Ok(group) => group,
            Err(error) => {
                table.release_buffer(uniform)?;
                return Err(error.into());
            }
        };
        Ok(Self {
            pipeline,
            format,
            uniform,
            group,
        })
    }

    pub(super) fn prepare(
        &mut self,
        table: &mut ResourceTable,
        format: wgpu::TextureFormat,
        camera: SceneCamera,
        size: [u32; 2],
    ) -> Result<(), RenderError> {
        if self.format != format {
            self.pipeline = create_pipeline(table, format)?;
            self.format = format;
        }
        let view = Mat3::from_mat4(camera.view);
        let view = if view.determinant() != 0.0 {
            Mat4::from_mat3(view)
        } else {
            Mat4::IDENTITY
        };
        let world_from_clip = SceneCamera { view, ..camera }
            .clip_from_world(size)
            .ok_or_else(|| RenderError("invalid skybox camera".into()))?
            .inverse();
        let mut bytes = [0; UNIFORM_SIZE.get() as usize];
        for (slot, value) in bytes
            .chunks_exact_mut(4)
            .zip(world_from_clip.to_cols_array())
        {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        table.write_buffer(self.uniform, 0, &bytes)?;
        Ok(())
    }

    pub(super) fn encode(
        &self,
        table: &ResourceTable,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        clear: wgpu::Color,
        label: &str,
    ) -> Result<(), RenderError> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, table.gpu_uniform_group(self.group)?, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

fn create_pipeline(
    table: &mut ResourceTable,
    format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, RenderError> {
    let uniforms = table.uniform_layout(1)?.clone();
    super::create_pipeline(
        &table.device,
        ScenePipelineKey::skybox(format),
        &[Some(&uniforms)],
    )
}
