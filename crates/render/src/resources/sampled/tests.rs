use std::sync::mpsc;
use std::time::Duration;

use super::*;

const COLOR_TEXTURE: TextureDescriptor = TextureDescriptor {
    size: [4, 2],
    mip_levels: 2,
    array_layers: 2,
    format: TextureFormat::Rgba8Unorm,
    view: TextureViewDescriptor {
        sample_type: TextureSampleType::FilterableFloat,
        dimension: TextureViewDimension::D2Array,
        base_mip_level: 1,
        mip_level_count: 1,
        base_array_layer: 1,
        array_layer_count: 1,
    },
};
const NEAREST: SamplerDescriptor = SamplerDescriptor {
    kind: SamplerKind::Filtering,
    filter: SamplerFilter::Nearest,
    address_mode: SamplerAddressMode::ClampToEdge,
};

#[test]
fn texture_view_contract_is_validated_against_limits_and_format() {
    let limits = wgpu::Limits::default();
    let validate = |descriptor| validate_texture(descriptor, &limits, wgpu::Features::empty());
    assert_eq!(validate(COLOR_TEXTURE), Ok(()));
    for descriptor in [
        TextureDescriptor {
            size: [0, 0],
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            size: [limits.max_texture_dimension_2d + 1, 1],
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            mip_levels: 4,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            mip_levels: 1,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            array_layers: 1,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            format: TextureFormat::R32Float,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            format: TextureFormat::Depth16Unorm,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            view: TextureViewDescriptor {
                base_mip_level: u32::MAX,
                ..COLOR_TEXTURE.view
            },
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            view: TextureViewDescriptor {
                array_layer_count: 0,
                ..COLOR_TEXTURE.view
            },
            ..COLOR_TEXTURE
        },
    ] {
        assert!(matches!(
            validate(descriptor),
            Err(ResourceError::InvalidDescriptor(_))
        ));
    }
    let view = TextureViewDescriptor {
        sample_type: TextureSampleType::UnfilterableFloat,
        ..COLOR_TEXTURE.view
    };
    assert_eq!(
        validate(TextureDescriptor {
            view,
            ..COLOR_TEXTURE
        }),
        Ok(())
    );
    assert_eq!(
        validate(TextureDescriptor {
            format: TextureFormat::R32Float,
            view,
            ..COLOR_TEXTURE
        }),
        Ok(())
    );
}

#[test]
fn non_filtering_sampler_requires_nearest() {
    let descriptor = SamplerDescriptor {
        kind: SamplerKind::NonFiltering,
        ..NEAREST
    };
    assert_eq!(descriptor.validate(), Ok(()));
    assert!(matches!(
        SamplerDescriptor {
            filter: SamplerFilter::Linear,
            ..descriptor
        }
        .validate(),
        Err(ResourceError::InvalidDescriptor(_))
    ));
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn classic_sampled_groups_preserve_view_and_binding_across_updates() {
    let mut table = open_device();
    let texture = table.create_texture(COLOR_TEXTURE).unwrap();
    let sampler = table.create_sampler(NEAREST).unwrap();
    let binding = SampledBinding { texture, sampler };
    let group = table.create_sampled_group(binding).unwrap();
    let duplicate = table.create_sampled_group(binding).unwrap();
    let shader = "
        @group(0) @binding(0) var image: texture_2d_array<f32>;
        @group(0) @binding(1) var image_sampler: sampler;
        @fragment fn sample_color() -> @location(0) vec4f {
            return textureSampleLevel(image, image_sampler, vec2f(0.375, 0.5), 0, 0.0);
        }
    ";
    let probe = SamplingProbe::new(&table, group, shader, "sample_color");
    let original = table.sampled_group(group).unwrap().raw.clone();
    let duplicate_original = table.sampled_group(duplicate).unwrap().raw.clone();

    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    table.write_texture(texture, 1, 0, &blue.repeat(2)).unwrap();
    table.write_texture(texture, 0, 1, &blue.repeat(8)).unwrap();
    table.write_texture(texture, 1, 1, &red.repeat(2)).unwrap();
    assert_eq!(probe.sample(&table, group), red);

    table.write_texture(texture, 1, 1, &blue.repeat(2)).unwrap();
    assert_eq!(table.sampled_group(group).unwrap().raw, original);
    assert_eq!(probe.sample(&table, group), blue);
    for (mip, layer, bytes) in [
        (2, 1, &[0; 8][..]),
        (1, 2, &[0; 8][..]),
        (1, 1, &[0; 7][..]),
    ] {
        assert!(matches!(
            table.write_texture(texture, mip, layer, bytes),
            Err(ResourceError::InvalidRange(_))
        ));
    }

    for bad in [
        TextureDescriptor {
            array_layers: 1,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            format: TextureFormat::R32Float,
            ..COLOR_TEXTURE
        },
        TextureDescriptor {
            view: TextureViewDescriptor {
                base_array_layer: 0,
                ..COLOR_TEXTURE.view
            },
            ..COLOR_TEXTURE
        },
    ] {
        assert!(matches!(
            table.replace_texture(texture, bad),
            Err(ResourceError::IncompatibleReplacement(_))
        ));
    }
    assert_eq!(table.texture_descriptor(texture), Ok(COLOR_TEXTURE));
    assert_eq!(table.sampled_group(group).unwrap().raw, original);
    assert_eq!(probe.sample(&table, group), blue);

    let replacement = TextureDescriptor {
        array_layers: 3,
        ..COLOR_TEXTURE
    };
    table.replace_texture(texture, replacement).unwrap();
    assert_eq!(table.texture_descriptor(texture), Ok(replacement));
    assert_eq!(table.sampled_binding(group), Ok(binding));
    assert_ne!(table.sampled_group(group).unwrap().raw, original);
    assert_ne!(
        table.sampled_group(duplicate).unwrap().raw,
        duplicate_original
    );
    table.release_sampled_group(duplicate).unwrap();
    assert_eq!(
        table.sampled_binding(duplicate),
        Err(ResourceError::StaleHandle)
    );
    table
        .write_texture(texture, 1, 1, &[red, blue].concat())
        .unwrap();
    assert_eq!(probe.sample(&table, group), red);

    let invalid_sampler = SamplerDescriptor {
        kind: SamplerKind::NonFiltering,
        ..NEAREST
    };
    let prior_group = table.sampled_group(group).unwrap().raw.clone();
    assert!(matches!(
        table.replace_sampler(sampler, invalid_sampler),
        Err(ResourceError::IncompatibleReplacement(_))
    ));
    assert_eq!(table.sampler_descriptor(sampler), Ok(NEAREST));
    assert_eq!(table.sampled_group(group).unwrap().raw, prior_group);
    let linear = SamplerDescriptor {
        filter: SamplerFilter::Linear,
        ..NEAREST
    };
    table.replace_sampler(sampler, linear).unwrap();
    assert_eq!(table.sampler_descriptor(sampler), Ok(linear));
    assert_eq!(table.sampled_binding(group), Ok(binding));
    assert_ne!(table.sampled_group(group).unwrap().raw, prior_group);
    assert_eq!(probe.sample(&table, group), [191, 0, 64, 255]);

    let mut foreign = ResourceTable::new(table.device.clone(), table.queue.clone());
    assert_eq!(
        foreign.write_texture(texture, 1, 1, &red),
        Err(ResourceError::ForeignHandle)
    );
    assert_eq!(
        foreign.replace_sampler(sampler, NEAREST),
        Err(ResourceError::ForeignHandle)
    );
    assert_eq!(
        foreign.sampled_binding(group),
        Err(ResourceError::ForeignHandle)
    );
    table.release_texture(texture).unwrap();
    assert_eq!(
        table.sampled_binding(group),
        Err(ResourceError::StaleHandle)
    );
    assert_eq!(
        table.write_texture(texture, 1, 1, &red),
        Err(ResourceError::StaleHandle)
    );
    let recycled = table.create_texture(COLOR_TEXTURE).unwrap();
    assert_ne!(recycled, texture);
    let group = table
        .create_sampled_group(SampledBinding {
            texture: recycled,
            sampler,
        })
        .unwrap();
    table.release_sampler(sampler).unwrap();
    assert_eq!(
        table.sampled_binding(group),
        Err(ResourceError::StaleHandle)
    );
    assert_eq!(
        table.replace_sampler(sampler, NEAREST),
        Err(ResourceError::StaleHandle)
    );
    assert_ne!(table.create_sampler(NEAREST).unwrap(), sampler);
}

#[test]
#[ignore = "requires a native GPU adapter with depth texture copies"]
fn classic_comparison_group_preserves_depth_sampler_contract() {
    let mut table = open_device();
    let texture = table
        .create_texture(TextureDescriptor {
            size: [1, 1],
            mip_levels: 1,
            array_layers: 1,
            format: TextureFormat::Depth16Unorm,
            view: TextureViewDescriptor {
                sample_type: TextureSampleType::Depth,
                dimension: TextureViewDimension::D2,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        })
        .unwrap();
    let comparison = SamplerDescriptor {
        kind: SamplerKind::Comparison(CompareFunction::Less),
        ..NEAREST
    };
    let sampler = table.create_sampler(comparison).unwrap();
    let group = table
        .create_sampled_group(SampledBinding { texture, sampler })
        .unwrap();
    let color = table.create_texture(COLOR_TEXTURE).unwrap();
    assert!(matches!(
        table.create_sampled_group(SampledBinding {
            texture: color,
            sampler
        }),
        Err(ResourceError::InvalidDescriptor(_))
    ));
    let shader = "
        @group(0) @binding(0) var depth: texture_depth_2d;
        @group(0) @binding(1) var compare: sampler_comparison;
        @fragment fn compare_depth() -> @location(0) vec4f {
            let result = textureSampleCompareLevel(depth, compare, vec2f(0.5), 0.5);
            return vec4f(result, 0.0, 0.0, 1.0);
        }
    ";
    let probe = SamplingProbe::new(&table, group, shader, "compare_depth");
    table
        .write_texture(texture, 0, 0, &49152_u16.to_le_bytes())
        .unwrap();
    assert_eq!(probe.sample(&table, group), [255, 0, 0, 255]);
    table
        .replace_sampler(
            sampler,
            SamplerDescriptor {
                kind: SamplerKind::Comparison(CompareFunction::Greater),
                ..comparison
            },
        )
        .unwrap();
    assert_eq!(probe.sample(&table, group), [0, 0, 0, 255]);
}

fn open_device() -> ResourceTable {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .unwrap();
    assert!(device.features().is_empty());
    let info = adapter.get_info();
    eprintln!(
        "classic sampled groups on {} ({:?})",
        info.name, info.backend
    );
    ResourceTable::new(device, queue)
}

struct SamplingProbe {
    pipeline: wgpu::RenderPipeline,
    output: wgpu::Texture,
}

impl SamplingProbe {
    fn new(table: &ResourceTable, group: SampledGroupHandle, shader: &str, entry: &str) -> Self {
        let device = &table.device;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let vertex = "
            @vertex fn triangle(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
                let positions = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
                return vec4f(positions[vertex], 0.0, 1.0);
            }
        ";
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(format!("{vertex}\n{shader}").into()),
        });
        let group = table.sampled_group(group).unwrap();
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&group.layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("triangle"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self { pipeline, output }
    }

    fn sample(&self, table: &ResourceTable, group: SampledGroupHandle) -> [u8; 4] {
        let readback = table.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let view = self.output.create_view(&Default::default());
        let mut encoder = table.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &table.sampled_group(group).unwrap().raw, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: Default::default(),
            },
            self.output.size(),
        );
        table.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (send, recv) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
        table
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        recv.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        (*slice.get_mapped_range().unwrap()).try_into().unwrap()
    }
}
