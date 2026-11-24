use std::sync::mpsc;
use std::time::Duration;

use super::{
    BufferDescriptor, BufferKind, ResourceError, ResourceTable, UniformBinding, UniformGroupHandle,
    range_fits,
};

#[test]
fn buffer_ranges_fit_without_wrapping() {
    assert!(range_fits(32, 16, 48));
    assert!(range_fits(48, 0, 48));
    assert!(!range_fits(32, 17, 48));
    assert!(!range_fits(u64::MAX - 3, 4, u64::MAX));
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn classic_uniform_groups_preserve_contents_and_handles_across_frame_updates() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .unwrap();
    assert!(device.features().is_empty());
    let mut table = ResourceTable::new(device, queue);

    for kind in [BufferKind::Vertex, BufferKind::Index] {
        let descriptor = BufferDescriptor { kind, size: 8 };
        let handle = table.create_buffer(descriptor).unwrap();
        assert_eq!(table.buffer_descriptor(handle), Ok(descriptor));
        table.write_buffer(handle, 0, &[1; 8]).unwrap();
        table.release_buffer(handle).unwrap();
        assert_eq!(
            table.buffer_descriptor(handle),
            Err(ResourceError::StaleHandle)
        );
    }

    let offset = table.uniform_offset_alignment();
    let descriptor = BufferDescriptor {
        kind: BufferKind::Uniform,
        size: offset + 16,
    };
    let uniform = table.create_buffer(descriptor).unwrap();
    let range = UniformBinding {
        buffer: uniform,
        offset,
        size: 16.try_into().unwrap(),
    };
    let group = table.create_uniform_group(&[range]).unwrap();
    let duplicate = table.create_uniform_group(&[range]).unwrap();
    let probe = UniformProbe::new(&table);

    let original_group = table.groups.get(group.0.key).unwrap().raw.clone();
    let original_duplicate = table.groups.get(duplicate.0.key).unwrap().raw.clone();
    table
        .write_buffer(uniform, offset, &12_u32.to_le_bytes())
        .unwrap();
    let before_upload = probe.capture(&table, group);
    table
        .write_buffer(uniform, offset, &29_u32.to_le_bytes())
        .unwrap();
    assert_eq!(table.groups.get(group.0.key).unwrap().raw, original_group);
    let uploaded = probe.capture(&table, group);

    let too_small = BufferDescriptor {
        size: offset + 8,
        ..descriptor
    };
    assert!(matches!(
        table.replace_buffer(uniform, too_small),
        Err(ResourceError::IncompatibleReplacement(_))
    ));
    assert_eq!(table.buffer_descriptor(uniform), Ok(descriptor));
    assert_eq!(table.groups.get(group.0.key).unwrap().raw, original_group);
    assert!(matches!(
        table.write_buffer(uniform, offset + 1, &[0; 4]),
        Err(ResourceError::InvalidRange(_))
    ));
    let rejected = probe.capture(&table, group);

    let replacement = BufferDescriptor {
        size: offset + 32,
        ..descriptor
    };
    table.replace_buffer(uniform, replacement).unwrap();
    assert_eq!(table.buffer_descriptor(uniform), Ok(replacement));
    assert_eq!(table.uniform_bindings(group), Ok([range].as_slice()));
    assert_ne!(table.groups.get(group.0.key).unwrap().raw, original_group);
    assert_ne!(
        table.groups.get(duplicate.0.key).unwrap().raw,
        original_duplicate
    );
    table.release_uniform_group(duplicate).unwrap();
    assert_eq!(
        table.uniform_bindings(duplicate),
        Err(ResourceError::StaleHandle)
    );
    table
        .write_buffer(uniform, offset, &47_u32.to_le_bytes())
        .unwrap();
    let replaced = probe.capture(&table, group);
    drop(original_group);

    table.release_buffer(uniform).unwrap();
    assert_eq!(
        table.uniform_bindings(group),
        Err(ResourceError::StaleHandle)
    );
    let recycled = table.create_buffer(descriptor).unwrap();
    assert_ne!(recycled, uniform);
    assert_eq!(
        table.buffer_descriptor(uniform),
        Err(ResourceError::StaleHandle)
    );
    let mut other = ResourceTable::new(table.device.clone(), table.queue.clone());
    assert_eq!(
        other.write_buffer(recycled, 0, &[0; 4]),
        Err(ResourceError::ForeignHandle)
    );
    assert_eq!(
        other.release_uniform_group(group),
        Err(ResourceError::ForeignHandle)
    );

    for (capture, expected) in [
        (before_upload, 12),
        (uploaded, 29),
        (rejected, 29),
        (replaced, 47),
    ] {
        let slice = capture.slice(..);
        let (send, recv) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
        table
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        recv.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        let bytes = slice.get_mapped_range().unwrap();
        assert_eq!(u32::from_le_bytes((*bytes).try_into().unwrap()), expected);
    }
    let info = adapter.get_info();
    eprintln!(
        "classic uniform readbacks passed on {} ({:?})",
        info.name, info.backend
    );
}

struct UniformProbe {
    pipeline: wgpu::ComputePipeline,
    storage: wgpu::Buffer,
    output: wgpu::BindGroup,
}

impl UniformProbe {
    fn new(table: &ResourceTable) -> Self {
        let device = &table.device;
        let storage = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let output_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let output = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &output_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: storage.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&table.layouts[&1]), Some(&output_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                r"
                @group(0) @binding(0) var<uniform> input: vec4<u32>;
                @group(1) @binding(0) var<storage, read_write> output: u32;

                @compute @workgroup_size(1)
                fn read_uniform() { output = input.x; }
            "
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&layout),
            module: &module,
            entry_point: Some("read_uniform"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            pipeline,
            storage,
            output,
        }
    }

    fn capture(&self, table: &ResourceTable, group: UniformGroupHandle) -> wgpu::Buffer {
        let readback = table.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = table.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &table.groups.get(group.0.key).unwrap().raw, &[]);
            pass.set_bind_group(1, &self.output, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.storage, 0, &readback, 0, 4);
        table.queue.submit([encoder.finish()]);
        readback
    }
}
