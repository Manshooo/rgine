use super::{Handle, ResourceError, ResourceTable, SlotKey, Slots, checked_gpu, range_fits};
use std::collections::HashMap;

mod bindless;
#[cfg(test)]
mod tests;

pub(super) use bindless::BindlessTable;

/// A sampled texture view owned by one device's resource table.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TextureHandle(Handle);

/// A sampler owned by one device's resource table.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SamplerHandle(Handle);

/// A classic sampled bind group owned by one device's resource table.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SampledGroupHandle(Handle);

/// Backing format of a sampled texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureFormat {
    /// Four normalized 8-bit channels.
    Rgba8Unorm,
    /// Four normalized 8-bit channels with sRGB decoding for RGB.
    Rgba8UnormSrgb,
    /// One 32-bit floating-point channel.
    R32Float,
    /// One normalized 16-bit depth channel.
    Depth16Unorm,
}

impl TextureFormat {
    fn gpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
            Self::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::R32Float => wgpu::TextureFormat::R32Float,
            Self::Depth16Unorm => wgpu::TextureFormat::Depth16Unorm,
        }
    }
}

/// Sample type declared in the shader binding layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TextureSampleType {
    /// Color samples that may be filtered.
    FilterableFloat,
    /// Color samples that require a non-filtering sampler.
    UnfilterableFloat,
    /// Depth samples, including use with a comparison sampler.
    Depth,
}

impl TextureSampleType {
    fn gpu(self) -> wgpu::TextureSampleType {
        match self {
            Self::FilterableFloat => wgpu::TextureSampleType::Float { filterable: true },
            Self::UnfilterableFloat => wgpu::TextureSampleType::Float { filterable: false },
            Self::Depth => wgpu::TextureSampleType::Depth,
        }
    }
}

/// Shape expected by the sampled binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TextureViewDimension {
    /// One layer of a 2D texture.
    D2,
    /// A 2D array of one or more layers.
    D2Array,
}

impl TextureViewDimension {
    fn gpu(self) -> wgpu::TextureViewDimension {
        match self {
            Self::D2 => wgpu::TextureViewDimension::D2,
            Self::D2Array => wgpu::TextureViewDimension::D2Array,
        }
    }
}

/// The sampled binding contract and the subresources visible through the handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextureViewDescriptor {
    /// Sample type used for the binding layout.
    pub sample_type: TextureSampleType,
    /// View dimension used for the binding layout.
    pub dimension: TextureViewDimension,
    /// First visible mip level.
    pub base_mip_level: u32,
    /// Number of visible mip levels, starting at the base.
    pub mip_level_count: u32,
    /// First visible array layer.
    pub base_array_layer: u32,
    /// Number of visible array layers, starting at the base.
    pub array_layer_count: u32,
}

/// Backing allocation and view for a single-sampled 2D texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextureDescriptor {
    /// Width and height in texels at mip level zero.
    pub size: [u32; 2],
    /// Total mip level count.
    pub mip_levels: u32,
    /// Total backing array layer count.
    pub array_layers: u32,
    /// Backing texel format.
    pub format: TextureFormat,
    /// Subresources exposed to the shader.
    pub view: TextureViewDescriptor,
}

impl TextureDescriptor {
    pub(crate) fn sampled_rgba8(size: [u32; 2]) -> Self {
        Self {
            size,
            mip_levels: 1,
            array_layers: 1,
            format: TextureFormat::Rgba8Unorm,
            view: TextureViewDescriptor {
                sample_type: TextureSampleType::FilterableFloat,
                dimension: TextureViewDimension::D2,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        }
    }
}

/// Comparison operation for a depth sampler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompareFunction {
    /// The reference value is strictly less than the sampled depth.
    Less,
    /// The reference value is less than or equal to the sampled depth.
    LessEqual,
    /// The reference value is strictly greater than the sampled depth.
    Greater,
    /// The reference value is greater than or equal to the sampled depth.
    GreaterEqual,
}

impl CompareFunction {
    fn gpu(self) -> wgpu::CompareFunction {
        match self {
            Self::Less => wgpu::CompareFunction::Less,
            Self::LessEqual => wgpu::CompareFunction::LessEqual,
            Self::Greater => wgpu::CompareFunction::Greater,
            Self::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
        }
    }
}

/// Sampler binding role preserved across replacements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerKind {
    /// May interpolate color samples.
    Filtering,
    /// Uses nearest-neighbor sampling only.
    NonFiltering,
    /// Compares a shader-provided reference with sampled depth.
    Comparison(CompareFunction),
}

impl SamplerKind {
    fn binding_type(self) -> wgpu::SamplerBindingType {
        match self {
            Self::Filtering => wgpu::SamplerBindingType::Filtering,
            Self::NonFiltering => wgpu::SamplerBindingType::NonFiltering,
            Self::Comparison(_) => wgpu::SamplerBindingType::Comparison,
        }
    }
}

/// Filter for texel and mip selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerFilter {
    /// Select the nearest sample.
    Nearest,
    /// Interpolate adjacent samples.
    Linear,
}

/// Addressing behavior along both texture axes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerAddressMode {
    /// Clamp coordinates to the edge.
    ClampToEdge,
    /// Repeat the texture beyond its edges.
    Repeat,
    /// Repeat the texture with alternating mirrored copies.
    MirrorRepeat,
}

/// Sampling options for a classic or bindless sampled binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplerDescriptor {
    /// Binding role and comparison operation, where applicable.
    pub kind: SamplerKind,
    /// Filter used for minification, magnification and mip selection.
    pub filter: SamplerFilter,
    /// Addressing behavior along both axes.
    pub address_mode: SamplerAddressMode,
}

impl SamplerDescriptor {
    pub(crate) const NEAREST_CLAMP: Self = Self {
        kind: SamplerKind::Filtering,
        filter: SamplerFilter::Nearest,
        address_mode: SamplerAddressMode::ClampToEdge,
    };
}

/// The entries in a classic group, in shader binding order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampledBinding {
    /// Texture view at binding zero.
    pub texture: TextureHandle,
    /// Sampler at binding one.
    pub sampler: SamplerHandle,
}

struct Texture {
    descriptor: TextureDescriptor,
    raw: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Sampler {
    descriptor: SamplerDescriptor,
    raw: wgpu::Sampler,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct LayoutKey {
    sample_type: TextureSampleType,
    dimension: TextureViewDimension,
    sampler: wgpu::SamplerBindingType,
}

pub(crate) struct SampledGroup {
    binding: SampledBinding,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) raw: wgpu::BindGroup,
}

#[derive(Default)]
pub(super) struct SampledResources {
    textures: Slots<Texture>,
    samplers: Slots<Sampler>,
    groups: Slots<SampledGroup>,
    layouts: HashMap<LayoutKey, wgpu::BindGroupLayout>,
}

impl ResourceTable {
    /// Allocates a texture and its declared view.
    pub fn create_texture(
        &mut self,
        descriptor: TextureDescriptor,
    ) -> Result<TextureHandle, ResourceError> {
        validate_texture(descriptor, &self.limits, self.device.features())?;
        let texture = checked_gpu(&self.device, || self.make_texture(descriptor))?;
        let key = self.sampled.textures.insert(texture);
        Ok(TextureHandle(self.handle(key)))
    }

    /// Returns the backing and view descriptors of a live texture.
    pub fn texture_descriptor(
        &self,
        handle: TextureHandle,
    ) -> Result<TextureDescriptor, ResourceError> {
        Ok(self.texture(handle)?.descriptor)
    }

    /// Queues tightly packed texels for one complete mip level and array layer.
    /// Existing groups keep binding the same view. Mip and layer indices are
    /// relative to the backing allocation. Depth uploads require backend
    /// support for depth texture copies.
    pub fn write_texture(
        &mut self,
        handle: TextureHandle,
        mip_level: u32,
        array_layer: u32,
        data: &[u8],
    ) -> Result<(), ResourceError> {
        let texture = self.texture(handle)?;
        let (size, bytes_per_row) = texture.upload_layout(mip_level, array_layer, data.len())?;
        checked_gpu(&self.device, || {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture.raw,
                    mip_level,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: array_layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: None,
                },
                size,
            );
        })
    }

    /// Replaces texture backing while preserving the view contract and live group handles.
    /// Upload any required contents before sampling the new allocation.
    pub fn replace_texture(
        &mut self,
        handle: TextureHandle,
        descriptor: TextureDescriptor,
    ) -> Result<(), ResourceError> {
        let prior = self.texture(handle)?.descriptor;
        if prior.view != descriptor.view {
            return Err(ResourceError::IncompatibleReplacement(
                "texture replacement must retain the sampled view type and range".into(),
            ));
        }
        validate_texture(descriptor, &self.limits, self.device.features())
            .map_err(|error| ResourceError::IncompatibleReplacement(error.to_string()))?;
        let (texture, groups) = checked_gpu(&self.device, || {
            let texture = self.make_texture(descriptor);
            let groups = self.rebuild_sampled_groups(
                |binding| binding.texture == handle,
                |_, sampler| (&texture.view, sampler),
            );
            (texture, groups)
        })?;
        *self
            .sampled
            .textures
            .get_mut(handle.0.key)
            .expect("validated texture") = texture;
        self.publish_sampled_groups(groups);
        if let Some(table) = &mut self.bindless {
            table.texture_changed(handle, false);
        }
        Ok(())
    }

    /// Releases a texture and invalidates its dependent group handles.
    pub fn release_texture(&mut self, handle: TextureHandle) -> Result<(), ResourceError> {
        self.texture(handle)?;
        self.remove_sampled_groups(|binding| binding.texture == handle);
        self.sampled.textures.remove(handle.0.key);
        if let Some(table) = &mut self.bindless {
            table.texture_changed(handle, true);
        }
        Ok(())
    }

    /// Allocates a sampler for the declared binding role.
    pub fn create_sampler(
        &mut self,
        descriptor: SamplerDescriptor,
    ) -> Result<SamplerHandle, ResourceError> {
        descriptor.validate()?;
        let sampler = checked_gpu(&self.device, || self.make_sampler(descriptor))?;
        let key = self.sampled.samplers.insert(sampler);
        Ok(SamplerHandle(self.handle(key)))
    }

    /// Returns the descriptor of a live sampler.
    pub fn sampler_descriptor(
        &self,
        handle: SamplerHandle,
    ) -> Result<SamplerDescriptor, ResourceError> {
        Ok(self.sampler(handle)?.descriptor)
    }

    /// Replaces a sampler while preserving its binding role and live group handles.
    pub fn replace_sampler(
        &mut self,
        handle: SamplerHandle,
        descriptor: SamplerDescriptor,
    ) -> Result<(), ResourceError> {
        let prior = self.sampler(handle)?.descriptor;
        if prior.kind.binding_type() != descriptor.kind.binding_type() {
            return Err(ResourceError::IncompatibleReplacement(
                "sampler replacement must retain the sampler binding role".into(),
            ));
        }
        descriptor.validate()?;
        let (sampler, groups) = checked_gpu(&self.device, || {
            let sampler = self.make_sampler(descriptor);
            let groups = self.rebuild_sampled_groups(
                |binding| binding.sampler == handle,
                |texture, _| (texture, &sampler.raw),
            );
            (sampler, groups)
        })?;
        *self
            .sampled
            .samplers
            .get_mut(handle.0.key)
            .expect("validated sampler") = sampler;
        self.publish_sampled_groups(groups);
        if let Some(table) = &mut self.bindless {
            table.sampler_changed(handle, false);
        }
        Ok(())
    }

    /// Releases a sampler and invalidates its dependent group handles.
    pub fn release_sampler(&mut self, handle: SamplerHandle) -> Result<(), ResourceError> {
        self.sampler(handle)?;
        self.remove_sampled_groups(|binding| binding.sampler == handle);
        self.sampled.samplers.remove(handle.0.key);
        if let Some(table) = &mut self.bindless {
            table.sampler_changed(handle, true);
        }
        Ok(())
    }

    /// Creates a classic fragment-stage group with one sampled view and one sampler.
    pub fn create_sampled_group(
        &mut self,
        binding: SampledBinding,
    ) -> Result<SampledGroupHandle, ResourceError> {
        let texture = self.texture(binding.texture)?;
        let sampler = self.sampler(binding.sampler)?;
        let view = texture.descriptor.view;
        let sampler_type = sampler.descriptor.kind.binding_type();
        if (sampler_type == wgpu::SamplerBindingType::Comparison
            && view.sample_type != TextureSampleType::Depth)
            || (sampler_type == wgpu::SamplerBindingType::Filtering
                && view.sample_type == TextureSampleType::UnfilterableFloat)
        {
            return Err(ResourceError::InvalidDescriptor(
                "the sampler binding role is incompatible with the texture sample type".into(),
            ));
        }
        let layout = self.sampled_layout(view, sampler_type)?.clone();
        let texture = self.texture(binding.texture)?;
        let sampler = self.sampler(binding.sampler)?;
        let raw = checked_gpu(&self.device, || {
            self.make_sampled_group(&layout, &texture.view, &sampler.raw)
        })?;
        let group = self.sampled.groups.insert(SampledGroup {
            binding,
            layout,
            raw,
        });
        Ok(SampledGroupHandle(self.handle(group)))
    }

    pub(crate) fn sampled_layout(
        &mut self,
        view: TextureViewDescriptor,
        sampler: wgpu::SamplerBindingType,
    ) -> Result<&wgpu::BindGroupLayout, ResourceError> {
        let key = LayoutKey {
            sample_type: view.sample_type,
            dimension: view.dimension,
            sampler,
        };
        if !self.sampled.layouts.contains_key(&key) {
            let layout = checked_gpu(&self.device, || key.create(&self.device))?;
            self.sampled.layouts.insert(key, layout);
        }
        Ok(&self.sampled.layouts[&key])
    }

    /// Returns the resources bound by a live classic sampled group.
    pub fn sampled_binding(
        &self,
        handle: SampledGroupHandle,
    ) -> Result<SampledBinding, ResourceError> {
        Ok(self.sampled_group(handle)?.binding)
    }

    /// Releases a classic sampled group without releasing its resources.
    pub fn release_sampled_group(
        &mut self,
        handle: SampledGroupHandle,
    ) -> Result<(), ResourceError> {
        let key = self.check_table(handle.0)?;
        self.sampled
            .groups
            .remove(key)
            .ok_or(ResourceError::StaleHandle)?;
        Ok(())
    }

    fn texture(&self, handle: TextureHandle) -> Result<&Texture, ResourceError> {
        let key = self.check_table(handle.0)?;
        self.sampled
            .textures
            .get(key)
            .ok_or(ResourceError::StaleHandle)
    }

    fn sampler(&self, handle: SamplerHandle) -> Result<&Sampler, ResourceError> {
        let key = self.check_table(handle.0)?;
        self.sampled
            .samplers
            .get(key)
            .ok_or(ResourceError::StaleHandle)
    }

    pub(crate) fn sampled_group(
        &self,
        handle: SampledGroupHandle,
    ) -> Result<&SampledGroup, ResourceError> {
        let key = self.check_table(handle.0)?;
        self.sampled
            .groups
            .get(key)
            .ok_or(ResourceError::StaleHandle)
    }

    fn make_texture(&self, descriptor: TextureDescriptor) -> Texture {
        let [width, height] = descriptor.size;
        let raw = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resource table texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: descriptor.array_layers,
            },
            mip_level_count: descriptor.mip_levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: descriptor.format.gpu(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let range = descriptor.view;
        let view = raw.create_view(&wgpu::TextureViewDescriptor {
            label: Some("resource table sampled view"),
            dimension: Some(range.dimension.gpu()),
            base_mip_level: range.base_mip_level,
            mip_level_count: Some(range.mip_level_count),
            base_array_layer: range.base_array_layer,
            array_layer_count: Some(range.array_layer_count),
            ..Default::default()
        });
        Texture {
            descriptor,
            raw,
            view,
        }
    }

    fn make_sampler(&self, descriptor: SamplerDescriptor) -> Sampler {
        let address = match descriptor.address_mode {
            SamplerAddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
            SamplerAddressMode::Repeat => wgpu::AddressMode::Repeat,
            SamplerAddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
        };
        let (filter, mipmap_filter) = match descriptor.filter {
            SamplerFilter::Nearest => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest),
            SamplerFilter::Linear => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear),
        };
        let raw = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("resource table sampler"),
            address_mode_u: address,
            address_mode_v: address,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter,
            compare: match descriptor.kind {
                SamplerKind::Comparison(function) => Some(function.gpu()),
                _ => None,
            },
            ..Default::default()
        });
        Sampler { descriptor, raw }
    }

    fn rebuild_sampled_groups<'a>(
        &'a self,
        affected: impl Fn(SampledBinding) -> bool,
        replace: impl Fn(
            &'a wgpu::TextureView,
            &'a wgpu::Sampler,
        ) -> (&'a wgpu::TextureView, &'a wgpu::Sampler),
    ) -> Vec<(SlotKey, wgpu::BindGroup)> {
        self.sampled
            .groups
            .iter()
            .filter(|(_, group)| affected(group.binding))
            .map(|(key, group)| {
                let texture = &self
                    .texture(group.binding.texture)
                    .expect("live texture")
                    .view;
                let sampler = &self
                    .sampler(group.binding.sampler)
                    .expect("live sampler")
                    .raw;
                let (texture, sampler) = replace(texture, sampler);
                (
                    key,
                    self.make_sampled_group(&group.layout, texture, sampler),
                )
            })
            .collect()
    }

    fn publish_sampled_groups(&mut self, rebuilt: Vec<(SlotKey, wgpu::BindGroup)>) {
        for (key, raw) in rebuilt {
            self.sampled
                .groups
                .get_mut(key)
                .expect("live dependent group")
                .raw = raw;
        }
    }

    fn remove_sampled_groups(&mut self, affected: impl Fn(SampledBinding) -> bool) {
        let keys: Vec<_> = self
            .sampled
            .groups
            .iter()
            .filter_map(|(key, group)| affected(group.binding).then_some(key))
            .collect();
        for key in keys {
            self.sampled.groups.remove(key);
        }
    }

    fn make_sampled_group(
        &self,
        layout: &wgpu::BindGroupLayout,
        texture: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resource table sampled group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

fn validate_texture(
    descriptor: TextureDescriptor,
    limits: &wgpu::Limits,
    features: wgpu::Features,
) -> Result<(), ResourceError> {
    let [width, height] = descriptor.size;
    let limit = limits.max_texture_dimension_2d;
    let max_mips = u32::BITS - width.max(height).leading_zeros();
    if width == 0
        || height == 0
        || width > limit
        || height > limit
        || !(1..=limits.max_texture_array_layers).contains(&descriptor.array_layers)
        || !(1..=max_mips).contains(&descriptor.mip_levels)
    {
        return Err(ResourceError::InvalidDescriptor(
            "texture size, mip count or layer count is outside the device limits".into(),
        ));
    }
    let view = descriptor.view;
    if view.mip_level_count == 0
        || view.array_layer_count == 0
        || (view.dimension == TextureViewDimension::D2 && view.array_layer_count != 1)
        || !range_fits(
            view.base_mip_level.into(),
            view.mip_level_count.into(),
            descriptor.mip_levels.into(),
        )
        || !range_fits(
            view.base_array_layer.into(),
            view.array_layer_count.into(),
            descriptor.array_layers.into(),
        )
    {
        return Err(ResourceError::InvalidDescriptor(
            "texture view range or dimension is invalid for the backing allocation".into(),
        ));
    }
    let actual = descriptor.format.gpu().sample_type(None, Some(features));
    let declared = view.sample_type.gpu();
    if actual != Some(declared)
        && !(actual == Some(wgpu::TextureSampleType::Float { filterable: true })
            && view.sample_type == TextureSampleType::UnfilterableFloat)
    {
        return Err(ResourceError::InvalidDescriptor(
            "texture format does not support the declared sample type".into(),
        ));
    }
    Ok(())
}

impl LayoutKey {
    fn create(self, device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("classic sampled layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: self.sample_type.gpu(),
                        view_dimension: self.dimension.gpu(),
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(self.sampler),
                    count: None,
                },
            ],
        })
    }
}

impl Texture {
    fn upload_layout(
        &self,
        mip: u32,
        layer: u32,
        byte_count: usize,
    ) -> Result<(wgpu::Extent3d, u32), ResourceError> {
        if mip >= self.descriptor.mip_levels || layer >= self.descriptor.array_layers {
            return Err(ResourceError::InvalidRange(
                "texture upload addresses a mip level or layer outside the allocation".into(),
            ));
        }
        let size = self
            .raw
            .size()
            .mip_level_size(mip, wgpu::TextureDimension::D2);
        let bytes_per_texel = self
            .raw
            .format()
            .block_copy_size(None)
            .expect("supported texel format");
        let bytes_per_row = size.width * bytes_per_texel;
        if byte_count as u64 != u64::from(bytes_per_row) * u64::from(size.height) {
            return Err(ResourceError::InvalidRange(
                "texture upload needs tightly packed bytes for one complete mip level and layer"
                    .into(),
            ));
        }
        Ok((
            wgpu::Extent3d {
                depth_or_array_layers: 1,
                ..size
            },
            bytes_per_row,
        ))
    }
}

impl SamplerDescriptor {
    fn validate(self) -> Result<(), ResourceError> {
        if self.kind == SamplerKind::NonFiltering && self.filter != SamplerFilter::Nearest {
            return Err(ResourceError::InvalidDescriptor(
                "a non-filtering sampler requires nearest filtering".into(),
            ));
        }
        Ok(())
    }
}
