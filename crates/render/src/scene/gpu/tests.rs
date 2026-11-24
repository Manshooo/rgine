use std::sync::mpsc;
use std::time::Duration;

use engine_core::glam::{Mat4, Vec3};

use super::*;
use crate::resources::ResourceError;
use crate::scene::SceneCamera;

#[test]
#[ignore = "requires a native GPU adapter"]
fn scene_depth_and_stable_meshes_survive_a_new_device() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let open_device = || {
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        ResourceTable::new(device, queue)
    };
    let mut meshes = MeshRegistry::default();
    let cube = meshes.insert(Mesh::cube());
    let mut scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: vec![
            SceneObject {
                mesh: cube,
                transform: Mat4::from_translation(Vec3::new(0.0, 0.0, -2.0)),
                color: [1.0, 0.0, 0.0, 1.0],
            },
            SceneObject {
                mesh: cube,
                transform: Mat4::from_translation(Vec3::new(0.0, 0.0, -4.0)),
                color: [0.0, 1.0, 0.0, 1.0],
            },
        ],
    };

    let mut table = open_device();
    let mut renderer = SceneRenderer::default();
    let first = center_pixel(&mut renderer, &mut table, &meshes, &scene);
    assert!(
        first[0] > 64 && first[1] == 0,
        "front triangle wins despite draw order {first:?}"
    );
    assert_eq!(renderer.meshes.len(), 1);
    let old_handle = renderer.meshes[&cube].vertices;

    let original_camera = scene.camera;
    scene.camera.as_mut().unwrap().view =
        Mat4::look_at_rh(Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -4.0), Vec3::Y);
    let panned = center_pixel(&mut renderer, &mut table, &meshes, &scene);
    assert!(
        panned[1] > 64 && panned[0] == 0,
        "camera movement reveals the far mesh {panned:?}"
    );
    scene.camera = original_camera;
    scene.objects[0].transform = Mat4::from_translation(Vec3::new(3.0, 0.0, -2.0));
    let changed = center_pixel(&mut renderer, &mut table, &meshes, &scene);
    assert!(
        changed[1] > 64 && changed[0] == 0,
        "moving the front mesh reveals the far mesh {changed:?}"
    );
    assert_eq!(renderer.meshes[&cube].vertices, old_handle);

    drop(renderer);
    drop(table);
    let mut new_table = open_device();
    let mut new_renderer = SceneRenderer::default();
    assert_eq!(
        new_table.buffer_descriptor(old_handle),
        Err(ResourceError::ForeignHandle)
    );
    let rebuilt = center_pixel(&mut new_renderer, &mut new_table, &meshes, &scene);
    assert_eq!(rebuilt, changed);
    assert_ne!(new_renderer.meshes[&cube].vertices, old_handle);
    eprintln!(
        "center RGBA with near mesh {first:?}, after camera movement {panned:?}, after mesh movement {changed:?}, on the new device {rebuilt:?}"
    );
}

fn center_pixel(
    renderer: &mut SceneRenderer,
    table: &mut ResourceTable,
    meshes: &MeshRegistry,
    scene: &Scene,
) -> [u8; 4] {
    const SIDE: u32 = 64;
    const ROW: u32 = SIDE * 4;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    renderer
        .prepare(table, meshes, scene, [SIDE, SIDE], format)
        .unwrap();
    let size = wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let color = table.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene readback color"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = table.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(ROW * SIDE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = table.device.create_command_encoder(&Default::default());
    renderer
        .encode(
            table,
            scene,
            &mut encoder,
            &color.create_view(&Default::default()),
            wgpu::Color::BLACK,
        )
        .unwrap();
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW),
                rows_per_image: None,
            },
        },
        size,
    );
    table.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
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
    let center = ((SIDE / 2) * (ROW + 4)) as usize;
    bytes[center..center + 4].try_into().unwrap()
}
