use std::num::NonZeroU32;

use super::{
    ResourceError, ResourceTable, SampledBinding, SamplerDescriptor, SamplerHandle, SamplerKind,
    TextureDescriptor, TextureHandle, TextureSampleType, TextureViewDimension, checked_gpu,
};
use crate::resources::{BINDLESS_SAMPLERS, BINDLESS_TEXTURES};

pub(in crate::resources) struct BindlessTable {
    fallback: SampledBinding,
    textures: [Option<TextureHandle>; BINDLESS_TEXTURES as usize],
    samplers: [Option<SamplerHandle>; BINDLESS_SAMPLERS as usize],
    layout: wgpu::BindGroupLayout,
    group: Option<wgpu::BindGroup>,
}

impl BindlessTable {
    pub(super) fn texture_changed(&mut self, handle: TextureHandle, release: bool) {
        if invalidate_slot(&mut self.textures, handle, release) {
            self.group = None;
        }
    }

    pub(super) fn sampler_changed(&mut self, handle: SamplerHandle, release: bool) {
        if invalidate_slot(&mut self.samplers, handle, release) {
            self.group = None;
        }
    }

    fn build_group(&self, table: &ResourceTable) -> Result<wgpu::BindGroup, ResourceError> {
        let textures = self
            .textures
            .iter()
            .map(|entry| {
                table
                    .texture(entry.unwrap_or(self.fallback.texture))
                    .map(|texture| &texture.view)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let samplers = self
            .samplers
            .iter()
            .map(|entry| {
                table
                    .sampler(entry.unwrap_or(self.fallback.sampler))
                    .map(|sampler| &sampler.raw)
            })
            .collect::<Result<Vec<_>, _>>()?;
        checked_gpu(&table.device, || {
            table.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bindless sampled resources"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureViewArray(&textures),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::SamplerArray(&samplers),
                    },
                ],
            })
        })
    }
}

impl ResourceTable {
    pub(in crate::resources) fn enable_bindless(&mut self) -> Result<(), ResourceError> {
        let texture = self.create_texture(TextureDescriptor::sampled_rgba8([1, 1]))?;
        self.write_texture(texture, 0, 0, &[255; 4])?;
        let sampler = self.create_sampler(SamplerDescriptor::NEAREST_CLAMP)?;
        let layout = checked_gpu(&self.device, || {
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("bindless sampled layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: NonZeroU32::new(BINDLESS_TEXTURES),
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: NonZeroU32::new(BINDLESS_SAMPLERS),
                        },
                    ],
                })
        })?;
        self.bindless = Some(BindlessTable {
            fallback: SampledBinding { texture, sampler },
            textures: [None; BINDLESS_TEXTURES as usize],
            samplers: [None; BINDLESS_SAMPLERS as usize],
            layout,
            group: None,
        });
        self.prepare_bindless()
    }

    pub(crate) fn bindless_indices(
        &mut self,
        binding: SampledBinding,
    ) -> Result<[u32; 2], ResourceError> {
        let view = self.texture(binding.texture)?.descriptor.view;
        let sampler = self.sampler(binding.sampler)?.descriptor;
        if view.sample_type != TextureSampleType::FilterableFloat
            || view.dimension != TextureViewDimension::D2
            || sampler.kind != SamplerKind::Filtering
        {
            return Err(ResourceError::InvalidDescriptor(
                "bindless materials require a filterable 2D view and a filtering sampler".into(),
            ));
        }
        let table = self.bindless.as_mut().ok_or_else(no_bindless)?;
        let texture_index = available_slot(&table.textures, binding.texture).ok_or_else(|| {
            ResourceError::InvalidDescriptor("bindless texture slots are full".into())
        })?;
        let sampler_index = available_slot(&table.samplers, binding.sampler).ok_or_else(|| {
            ResourceError::InvalidDescriptor("bindless sampler slots are full".into())
        })?;
        if table.textures[texture_index].is_none() || table.samplers[sampler_index].is_none() {
            table.textures[texture_index] = Some(binding.texture);
            table.samplers[sampler_index] = Some(binding.sampler);
            table.group = None;
        }
        Ok([texture_index as u32, sampler_index as u32])
    }

    pub(crate) fn prepare_bindless(&mut self) -> Result<(), ResourceError> {
        let table = self.bindless.as_ref().ok_or_else(no_bindless)?;
        if table.group.is_none() {
            let group = table.build_group(self)?;
            self.bindless.as_mut().expect("bindless table exists").group = Some(group);
        }
        Ok(())
    }

    pub(crate) fn bindless_layout(&self) -> Result<&wgpu::BindGroupLayout, ResourceError> {
        Ok(&self.bindless.as_ref().ok_or_else(no_bindless)?.layout)
    }

    pub(crate) fn bindless_group(&self) -> Result<&wgpu::BindGroup, ResourceError> {
        self.bindless
            .as_ref()
            .ok_or_else(no_bindless)?
            .group
            .as_ref()
            .ok_or_else(|| {
                ResourceError::Backend("bindless resources must be prepared before encoding".into())
            })
    }
}

fn no_bindless() -> ResourceError {
    ResourceError::InvalidDescriptor("this device uses classic sampled bindings".into())
}

fn available_slot<T: PartialEq>(slots: &[Option<T>], handle: T) -> Option<usize> {
    // Slot zero is reserved for the fallback material.
    slots
        .iter()
        .position(|entry| entry.as_ref() == Some(&handle))
        .or_else(|| {
            slots
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, entry)| entry.is_none())
                .map(|(index, _)| index)
        })
}

fn invalidate_slot<T: PartialEq>(slots: &mut [Option<T>], handle: T, release: bool) -> bool {
    let Some(entry) = slots
        .iter_mut()
        .find(|entry| entry.as_ref() == Some(&handle))
    else {
        return false;
    };
    if release {
        *entry = None;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_reuse_holes_without_moving_existing_indices_or_using_fallback() {
        let mut slots = [None, Some(1), None, Some(3)];
        assert_eq!(available_slot(&slots, 3), Some(3));
        assert_eq!(available_slot(&slots, 4), Some(2));
        assert!(invalidate_slot(&mut slots, 1, false));
        assert_eq!(available_slot(&slots, 1), Some(1));
        assert!(invalidate_slot(&mut slots, 1, true));
        assert_eq!(available_slot(&slots, 4), Some(1));
        assert_eq!(available_slot(&[None, Some(1)], 3), None);
    }

    #[test]
    #[ignore = "requires a GPU with the draw-uniform bindless profile"]
    fn bindless_table_preserves_slots_and_refreshes_changed_bindings() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let mut table = ResourceTable::request(&adapter, false).unwrap();
        let descriptor = TextureDescriptor::sampled_rgba8([1, 1]);
        let texture = table.create_texture(descriptor).unwrap();
        let sampler = table
            .create_sampler(SamplerDescriptor::NEAREST_CLAMP)
            .unwrap();
        let binding = SampledBinding { texture, sampler };
        let indices = table.bindless_indices(binding).unwrap();
        assert_eq!(indices, [1, 1]);
        table.prepare_bindless().unwrap();
        let original = table.bindless_group().unwrap().clone();

        table.write_texture(texture, 0, 0, &[0, 0, 0, 255]).unwrap();
        assert_eq!(table.bindless_group().unwrap(), &original);
        table.replace_texture(texture, descriptor).unwrap();
        assert!(table.bindless_group().is_err());
        assert_eq!(table.bindless_indices(binding).unwrap(), indices);
        table.prepare_bindless().unwrap();
        let updated = table.bindless_group().unwrap().clone();
        assert_ne!(updated, original);
        table
            .replace_sampler(
                sampler,
                SamplerDescriptor {
                    filter: crate::resources::SamplerFilter::Linear,
                    ..SamplerDescriptor::NEAREST_CLAMP
                },
            )
            .unwrap();
        assert!(table.bindless_group().is_err());
        assert_eq!(table.bindless_indices(binding).unwrap(), indices);
        table.prepare_bindless().unwrap();
        assert_ne!(table.bindless_group().unwrap(), &updated);

        table.release_texture(texture).unwrap();
        assert_eq!(
            table.bindless_indices(binding),
            Err(ResourceError::StaleHandle)
        );
        table.prepare_bindless().unwrap();
        assert!(table.bindless.as_ref().unwrap().textures[indices[0] as usize].is_none());
        let new_texture = table.create_texture(descriptor).unwrap();
        assert_eq!(
            table
                .bindless_indices(SampledBinding {
                    texture: new_texture,
                    sampler
                })
                .unwrap(),
            indices
        );
        table.release_sampler(sampler).unwrap();
        table.prepare_bindless().unwrap();
        assert!(table.bindless.as_ref().unwrap().samplers[indices[1] as usize].is_none());
    }
}
