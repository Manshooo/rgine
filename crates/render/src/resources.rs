//! Device-scoped buffer handles and classic uniform bindings.

mod slots;

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use slots::{SlotKey, Slots};

static NEXT_TABLE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct Handle {
    table: u64,
    key: SlotKey,
}

/// A buffer allocated by one render device's resource table.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BufferHandle(Handle);

/// A classic bind group allocated by one render device's resource table.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct UniformGroupHandle(Handle);

/// The binding role declared when creating a buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BufferKind {
    /// Per-vertex or per-instance data.
    Vertex,
    /// Indices into vertex data.
    Index,
    /// Data read by a shader through a uniform binding.
    Uniform,
}

/// The role and backing capacity of a device buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferDescriptor {
    /// The role preserved across compatible replacements.
    pub kind: BufferKind,
    /// The backing capacity in bytes.
    pub size: u64,
}

/// A uniform buffer range occupying one entry in a classic bind group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UniformBinding {
    /// The uniform buffer to read.
    pub buffer: BufferHandle,
    /// The start of the range in bytes, aligned to the device's uniform offset alignment.
    pub offset: u64,
    /// The length of the range in bytes.
    pub size: NonZeroU64,
}

/// A rejected resource operation.
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceError {
    /// The handle belongs to another device's table.
    ForeignHandle,
    /// The resource or bind group has been released.
    StaleHandle,
    /// A descriptor violates the resource contract or device limits.
    InvalidDescriptor(String),
    /// An upload or binding requests an invalid or misaligned range.
    InvalidRange(String),
    /// The replacement would invalidate the existing contract or a live binding.
    IncompatibleReplacement(String),
    /// The GPU backend rejected an allocation or bind group.
    Backend(String),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignHandle => f.write_str("resource handle belongs to another table"),
            Self::StaleHandle => f.write_str("resource handle has been released"),
            Self::InvalidDescriptor(message)
            | Self::InvalidRange(message)
            | Self::IncompatibleReplacement(message)
            | Self::Backend(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResourceError {}

struct Buffer {
    descriptor: BufferDescriptor,
    raw: wgpu::Buffer,
}

struct UniformGroup {
    bindings: Vec<UniformBinding>,
    raw: wgpu::BindGroup,
}

/// Owns buffers and bind groups for a single drawable device.
///
/// Update this table after submitting the previous frame and before encoding the next.
pub struct ResourceTable {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    id: u64,
    limits: wgpu::Limits,
    buffers: Slots<Buffer>,
    groups: Slots<UniformGroup>,
    layouts: HashMap<usize, wgpu::BindGroupLayout>,
}

impl ResourceTable {
    pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let mut id = NEXT_TABLE.load(Ordering::Relaxed);
        loop {
            let next = id
                .checked_add(1)
                .expect("resource table identities exhausted");
            match NEXT_TABLE.compare_exchange_weak(id, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(current) => id = current,
            }
        }
        Self {
            limits: device.limits(),
            device,
            queue,
            id,
            buffers: Slots::default(),
            groups: Slots::default(),
            layouts: HashMap::new(),
        }
    }

    /// Allocates backing storage for a buffer.
    pub fn create_buffer(
        &mut self,
        descriptor: BufferDescriptor,
    ) -> Result<BufferHandle, ResourceError> {
        self.validate_buffer(descriptor)?;
        let raw = checked_gpu(&self.device, || self.make_buffer(descriptor))?;
        let key = self.buffers.insert(Buffer { descriptor, raw });
        Ok(BufferHandle(self.handle(key)))
    }

    /// Returns the current descriptor of a live buffer.
    pub fn buffer_descriptor(
        &self,
        buffer: BufferHandle,
    ) -> Result<BufferDescriptor, ResourceError> {
        Ok(self.buffer(buffer)?.descriptor)
    }

    /// Queues an upload for the next submission. Offset and byte count must be
    /// multiples of four. Use padding for index data with an odd number of u16 indices.
    pub fn write_buffer(
        &mut self,
        handle: BufferHandle,
        offset: u64,
        data: &[u8],
    ) -> Result<(), ResourceError> {
        let buffer = self.buffer(handle)?;
        let size = data.len() as u64;
        if !range_fits(offset, size, buffer.descriptor.size)
            || !offset.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
            || !size.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
        {
            return Err(ResourceError::InvalidRange(format!(
                "upload at {offset} with length {size} must fit the buffer and align to four bytes"
            )));
        }
        if !data.is_empty() {
            self.queue.write_buffer(&buffer.raw, offset, data);
        }
        Ok(())
    }

    /// Replaces a buffer with fresh backing storage, preserving its handle and live bindings.
    /// Upload any required contents before the new storage is used.
    pub fn replace_buffer(
        &mut self,
        handle: BufferHandle,
        descriptor: BufferDescriptor,
    ) -> Result<(), ResourceError> {
        let prior = self.buffer(handle)?.descriptor;
        self.validate_buffer(descriptor)?;
        if prior.kind != descriptor.kind {
            return Err(ResourceError::IncompatibleReplacement(format!(
                "replacement must retain the {:?} buffer role",
                prior.kind
            )));
        }

        let affected: Vec<_> = self.groups_using(handle).collect();
        for (_, group) in &affected {
            for binding in group.bindings.iter().filter(|entry| entry.buffer == handle) {
                if !range_fits(binding.offset, binding.size.get(), descriptor.size) {
                    return Err(ResourceError::IncompatibleReplacement(format!(
                        "replacement of size {} cannot cover uniform range at {} with length {}",
                        descriptor.size, binding.offset, binding.size
                    )));
                }
            }
        }

        let (raw, rebuilt) = checked_gpu(&self.device, || {
            let buffer = self.make_buffer(descriptor);
            let groups = affected
                .iter()
                .map(|(key, group)| {
                    let layout = &self.layouts[&group.bindings.len()];
                    let raw = self.make_group(layout, &group.bindings, Some((handle, &buffer)));
                    (*key, raw)
                })
                .collect::<Vec<_>>();
            (buffer, groups)
        })?;

        // Publish only after both the backing and every dependent group are valid.
        *self
            .buffers
            .get_mut(handle.0.key)
            .expect("validated buffer") = Buffer { descriptor, raw };
        for (key, raw) in rebuilt {
            let group = self.groups.get_mut(key).expect("live dependent group");
            drop(std::mem::replace(&mut group.raw, raw));
        }
        Ok(())
    }

    /// Releases a buffer and invalidates the handles of groups that refer to it.
    pub fn release_buffer(&mut self, handle: BufferHandle) -> Result<(), ResourceError> {
        self.buffer(handle)?;
        let groups: Vec<_> = self.groups_using(handle).map(|(key, _)| key).collect();
        for key in groups {
            self.groups.remove(key);
        }
        self.buffers.remove(handle.0.key);
        Ok(())
    }

    /// Creates a classic group whose ordered ranges occupy shader bindings starting at zero.
    pub fn create_uniform_group(
        &mut self,
        bindings: &[UniformBinding],
    ) -> Result<UniformGroupHandle, ResourceError> {
        if bindings.is_empty()
            || bindings.len() > self.limits.max_uniform_buffers_per_shader_stage as usize
        {
            return Err(ResourceError::InvalidDescriptor(format!(
                "a uniform group needs between 1 and {} entries",
                self.limits.max_uniform_buffers_per_shader_stage
            )));
        }
        for binding in bindings {
            self.validate_uniform_binding(*binding)?;
        }

        let count = bindings.len();
        let (layout, raw) = checked_gpu(&self.device, || {
            let layout = self
                .layouts
                .get(&count)
                .cloned()
                .unwrap_or_else(|| self.make_layout(count));
            let raw = self.make_group(&layout, bindings, None);
            (layout, raw)
        })?;
        self.layouts.entry(count).or_insert(layout);
        let key = self.groups.insert(UniformGroup {
            bindings: bindings.to_vec(),
            raw,
        });
        Ok(UniformGroupHandle(self.handle(key)))
    }

    /// Returns the ordered ranges of a live uniform group.
    pub fn uniform_bindings(
        &self,
        handle: UniformGroupHandle,
    ) -> Result<&[UniformBinding], ResourceError> {
        let key = self.check_table(handle.0)?;
        let group = self.groups.get(key).ok_or(ResourceError::StaleHandle)?;
        Ok(&group.bindings)
    }

    /// Releases a bind group so its ranges no longer constrain buffer replacement.
    pub fn release_uniform_group(
        &mut self,
        handle: UniformGroupHandle,
    ) -> Result<(), ResourceError> {
        let key = self.check_table(handle.0)?;
        self.groups.remove(key).ok_or(ResourceError::StaleHandle)?;
        Ok(())
    }

    /// Returns the alignment required for uniform binding offsets in bytes.
    pub fn uniform_offset_alignment(&self) -> u64 {
        u64::from(self.limits.min_uniform_buffer_offset_alignment)
    }

    fn handle(&self, key: SlotKey) -> Handle {
        Handle {
            table: self.id,
            key,
        }
    }

    fn check_table(&self, handle: Handle) -> Result<SlotKey, ResourceError> {
        if handle.table != self.id {
            return Err(ResourceError::ForeignHandle);
        }
        Ok(handle.key)
    }

    fn buffer(&self, handle: BufferHandle) -> Result<&Buffer, ResourceError> {
        let key = self.check_table(handle.0)?;
        self.buffers.get(key).ok_or(ResourceError::StaleHandle)
    }

    fn groups_using(&self, handle: BufferHandle) -> impl Iterator<Item = (SlotKey, &UniformGroup)> {
        self.groups.iter().filter(move |(_, group)| {
            group
                .bindings
                .iter()
                .any(|binding| binding.buffer == handle)
        })
    }

    fn validate_buffer(&self, descriptor: BufferDescriptor) -> Result<(), ResourceError> {
        let limit = self.limits.max_buffer_size;
        if descriptor.size == 0 || descriptor.size > limit {
            return Err(ResourceError::InvalidDescriptor(format!(
                "buffer size {} must be between 1 and {limit} bytes",
                descriptor.size
            )));
        }
        Ok(())
    }

    fn validate_uniform_binding(&self, binding: UniformBinding) -> Result<(), ResourceError> {
        let buffer = self.buffer(binding.buffer)?;
        if buffer.descriptor.kind != BufferKind::Uniform {
            return Err(ResourceError::InvalidDescriptor(
                "uniform binding requires a uniform buffer".into(),
            ));
        }
        let size = binding.size.get();
        let alignment = self.uniform_offset_alignment();
        if !binding.offset.is_multiple_of(alignment)
            || size > self.limits.max_uniform_buffer_binding_size
            || !range_fits(binding.offset, size, buffer.descriptor.size)
        {
            return Err(ResourceError::InvalidRange(format!(
                "uniform range at {} with length {size} must fit the buffer and device limits and start on a {alignment}-byte boundary",
                binding.offset
            )));
        }
        Ok(())
    }

    fn make_buffer(&self, descriptor: BufferDescriptor) -> wgpu::Buffer {
        let role = match descriptor.kind {
            BufferKind::Vertex => wgpu::BufferUsages::VERTEX,
            BufferKind::Index => wgpu::BufferUsages::INDEX,
            BufferKind::Uniform => wgpu::BufferUsages::UNIFORM,
        };
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resource table buffer"),
            size: descriptor.size,
            usage: role | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn make_layout(&self, count: usize) -> wgpu::BindGroupLayout {
        let entries: Vec<_> = (0..count)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        self.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("classic uniform layout"),
                entries: &entries,
            })
    }

    fn make_group(
        &self,
        layout: &wgpu::BindGroupLayout,
        bindings: &[UniformBinding],
        replacement: Option<(BufferHandle, &wgpu::Buffer)>,
    ) -> wgpu::BindGroup {
        let entries: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| {
                let buffer = match replacement {
                    Some((handle, new)) if handle == binding.buffer => new,
                    _ => {
                        &self
                            .buffer(binding.buffer)
                            .expect("validated uniform buffer")
                            .raw
                    }
                };
                wgpu::BindGroupEntry {
                    binding: index as u32,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: binding.offset,
                        size: Some(binding.size),
                    }),
                }
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resource table uniform group"),
            layout,
            entries: &entries,
        })
    }
}

fn range_fits(offset: u64, size: u64, capacity: u64) -> bool {
    offset <= capacity && size <= capacity - offset
}

fn checked_gpu<T>(
    device: &wgpu::Device,
    operation: impl FnOnce() -> T,
) -> Result<T, ResourceError> {
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = operation();
    let validation = pollster::block_on(validation.pop());
    let internal = pollster::block_on(internal.pop());
    let memory = pollster::block_on(memory.pop());
    if let Some(error) = validation.or(internal).or(memory) {
        return Err(ResourceError::Backend(error.to_string()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
