use std::num::NonZeroU64;

use engine_core::glam::{Mat3, Mat4};

use crate::RenderError;
use crate::resources::{
    BufferDescriptor, BufferHandle, BufferKind, ResourceTable, UniformBinding, UniformGroupHandle,
};
use crate::scene::SceneCamera;

pub(super) const UNIFORM_SIZE: NonZeroU64 = NonZeroU64::new(64).unwrap();

pub(super) struct Skybox {
    uniform: BufferHandle,
    group: UniformGroupHandle,
}

impl Skybox {
    pub(super) fn new(table: &mut ResourceTable) -> Result<Self, RenderError> {
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
        Ok(Self { uniform, group })
    }

    pub(super) fn prepare(
        &mut self,
        table: &mut ResourceTable,
        camera: SceneCamera,
        size: [u32; 2],
    ) -> Result<(), RenderError> {
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
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(world_from_clip.to_cols_array())
        {
            *slot = value.to_le_bytes();
        }
        table.write_buffer(self.uniform, 0, &bytes)?;
        Ok(())
    }

    pub(super) fn encode(
        &self,
        table: &ResourceTable,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
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
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, table.gpu_uniform_group(self.group)?, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
