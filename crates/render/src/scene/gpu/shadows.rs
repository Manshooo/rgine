use std::collections::HashMap;

use engine_core::glam::{Mat4, Vec3};

use super::GpuMesh;
use crate::resources::{ResourceError, checked_gpu};
use crate::scene::{DirectionalLight, MeshKey, SceneObject};

pub(super) const MAP_SIZE: [u32; 2] = [1024, 1024];

pub(super) struct ShadowTarget {
    pub(super) view: wgpu::TextureView,
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) group: wgpu::BindGroup,
}

impl ShadowTarget {
    pub(super) fn new(
        device: &wgpu::Device,
        view: wgpu::TextureView,
    ) -> Result<Self, ResourceError> {
        let (layout, group) = checked_gpu(device, || {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("directional shadow lookup layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("directional shadow comparison"),
                compare: Some(wgpu::CompareFunction::LessEqual),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("directional shadow lookup"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            (layout, group)
        })?;
        Ok(Self {
            view,
            layout,
            group,
        })
    }
}

pub(super) fn clip_from_world(
    light: DirectionalLight,
    objects: &[SceneObject],
    meshes: &HashMap<MeshKey, GpuMesh>,
) -> Mat4 {
    let direction = light.direction_to_light;
    let up = if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = Mat4::look_to_rh(Vec3::ZERO, -direction, up);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for object in objects {
        let mesh = &meshes[&object.mesh];
        let transform = view * object.transform;
        let center = (mesh.bounds_min + mesh.bounds_max) * 0.5;
        let half = (mesh.bounds_max - mesh.bounds_min) * 0.5;
        let center = (transform * center.extend(1.0)).truncate();
        // Absolute basis columns give the light-space extents of the transformed mesh bounds.
        let extent = transform.x_axis.truncate().abs() * half.x
            + transform.y_axis.truncate().abs() * half.y
            + transform.z_axis.truncate().abs() * half.z;
        if center.is_finite() && extent.is_finite() {
            min = min.min(center - extent);
            max = max.max(center + extent);
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Mat4::IDENTITY;
    }
    // Padding also keeps the projection valid for planar geometry and collapsed transforms.
    min -= Vec3::splat(0.25);
    max += Vec3::splat(0.25);
    Mat4::orthographic_rh(min.x, max.x, min.y, max.y, -max.z, -min.z) * view
}
