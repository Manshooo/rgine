use super::{ResourceError, ResourceTable};

pub(crate) const BINDLESS_TEXTURES: u32 = 128;
pub(crate) const BINDLESS_SAMPLERS: u32 = 16;

/// The sampled-resource binding model fixed when a render device is created.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BindingMode {
    /// Bind an individual texture view and sampler per material.
    Classic,
    /// Index shared texture and sampler arrays with draw-uniform indices.
    Bindless,
}

fn device_descriptor(
    features: wgpu::Features,
    limits: &wgpu::Limits,
    force_classic: bool,
) -> (BindingMode, wgpu::DeviceDescriptor<'static>) {
    let mut descriptor = wgpu::DeviceDescriptor::default();
    // wgpu counts both arrays toward the general array limit. Keep the same
    // ordinary limits as the classic device, including its pipeline limits.
    let required_limits = wgpu::Limits {
        max_binding_array_elements_per_shader_stage: BINDLESS_TEXTURES + BINDLESS_SAMPLERS,
        max_binding_array_sampler_elements_per_shader_stage: BINDLESS_SAMPLERS,
        ..descriptor.required_limits.clone()
    };
    let required_features = wgpu::Features::TEXTURE_BINDING_ARRAY;
    if force_classic
        || !features.contains(required_features)
        || !required_limits.check_limits(limits)
    {
        return (BindingMode::Classic, descriptor);
    }
    descriptor.required_features = required_features;
    descriptor.required_limits = required_limits;
    (BindingMode::Bindless, descriptor)
}

impl ResourceTable {
    pub(crate) fn request(
        adapter: &wgpu::Adapter,
        force_classic: bool,
        gpu_timing: bool,
    ) -> Result<Self, ResourceError> {
        let features = adapter.features();
        let (mode, mut descriptor) = device_descriptor(features, &adapter.limits(), force_classic);
        if gpu_timing {
            descriptor.required_features |= features & wgpu::Features::TIMESTAMP_QUERY;
        }
        let (device, queue) =
            pollster::block_on(adapter.request_device(&descriptor)).map_err(|error| {
                ResourceError::Backend(format!(
                    "could not open a {mode:?} graphics device: {error}"
                ))
            })?;
        let mut table = Self::new(device, queue);
        if mode == BindingMode::Bindless {
            table.enable_bindless()?;
        }
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_the_exact_draw_uniform_array_profile() {
        let limits = wgpu::Limits {
            max_binding_array_elements_per_shader_stage: 144,
            max_binding_array_sampler_elements_per_shader_stage: 16,
            ..Default::default()
        };
        let feature = wgpu::Features::TEXTURE_BINDING_ARRAY;
        let (mode, descriptor) = device_descriptor(feature, &limits, false);
        assert_eq!(mode, BindingMode::Bindless);
        assert_eq!(descriptor.required_features, feature);
        assert_eq!(descriptor.required_limits, limits);
    }

    #[test]
    fn selects_classic_when_forced_or_any_requirement_is_missing() {
        let exact = wgpu::Limits {
            max_binding_array_elements_per_shader_stage: 144,
            max_binding_array_sampler_elements_per_shader_stage: 16,
            ..Default::default()
        };
        let feature = wgpu::Features::TEXTURE_BINDING_ARRAY;
        for (features, limits, force_classic) in [
            (feature, exact.clone(), true),
            (wgpu::Features::empty(), exact.clone(), false),
            (
                feature,
                wgpu::Limits {
                    max_binding_array_elements_per_shader_stage: 143,
                    ..exact.clone()
                },
                false,
            ),
            (
                feature,
                wgpu::Limits {
                    max_binding_array_sampler_elements_per_shader_stage: 15,
                    ..exact.clone()
                },
                false,
            ),
            (
                feature,
                wgpu::Limits {
                    max_bind_groups: 1,
                    ..exact.clone()
                },
                false,
            ),
            (
                feature,
                wgpu::Limits {
                    max_bindings_per_bind_group: 1,
                    ..exact.clone()
                },
                false,
            ),
            (
                feature,
                wgpu::Limits {
                    max_uniform_buffers_per_shader_stage: 0,
                    ..exact
                },
                false,
            ),
        ] {
            let (mode, descriptor) = device_descriptor(features, &limits, force_classic);
            assert_eq!(mode, BindingMode::Classic);
            assert_eq!(descriptor.required_features, wgpu::Features::empty());
            assert_eq!(descriptor.required_limits, wgpu::Limits::default());
        }
    }
}
