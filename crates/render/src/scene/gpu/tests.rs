use std::sync::mpsc;
use std::time::Duration;

use engine_core::glam::{Mat4, Vec3};

use super::*;
use crate::resources::ResourceError;
use crate::scene::SceneCamera;

const SIDE: u32 = 64;

#[test]
fn scene_pipeline_keys_include_binding_mode_and_layout() {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let classic = ScenePipelineKey::new(format, BindingMode::Classic);
    let bindless = ScenePipelineKey::new(format, BindingMode::Bindless);
    assert_ne!(classic, bindless);
    assert_ne!(classic.shader, bindless.shader);
    assert!(classic.shader.wgsl().is_some());
    assert!(bindless.shader.wgsl().is_some());
    assert_ne!(classic.layout, bindless.layout);
    assert_eq!(
        (bindless.layout.sampled_textures, bindless.layout.samplers),
        (128, 16)
    );
}

#[test]
#[ignore = "requires a GPU with the draw-uniform bindless profile"]
fn classic_and_bindless_draw_the_same_materials() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let select_adapter =
        || pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let adapter = select_adapter();
    let mut automatic = ResourceTable::request(&adapter, false).unwrap();
    let info = adapter.get_info();
    let limits = adapter.limits();
    eprintln!(
        "{} ({:?}): mode {:?}, arrays {}, sampler arrays {}",
        info.name,
        info.backend,
        automatic.binding_mode(),
        limits.max_binding_array_elements_per_shader_stage,
        limits.max_binding_array_sampler_elements_per_shader_stage
    );
    assert_eq!(automatic.binding_mode(), BindingMode::Bindless);
    assert_eq!(
        automatic.device.features(),
        wgpu::Features::TEXTURE_BINDING_ARRAY
    );
    let mut classic = ResourceTable::request(&select_adapter(), true).unwrap();
    assert_eq!(classic.binding_mode(), BindingMode::Classic);
    assert!(classic.device.features().is_empty());

    let mut meshes = MeshRegistry::default();
    let solid = meshes.insert(Mesh::cube());
    let checker = meshes.insert(
        Mesh::cube()
            .with_checkerboard()
            .with_pbr_material(PbrMaterial::new(0.25, 0.55)),
    );
    let scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: vec![
            SceneObject {
                mesh: solid,
                transform: Mat4::from_translation(Vec3::new(-0.7, 0.0, -3.0)),
                color: [0.0, 1.0, 0.0, 1.0],
            },
            SceneObject {
                mesh: checker,
                transform: Mat4::from_translation(Vec3::new(0.7, 0.0, -3.0)),
                color: [1.0, 0.0, 0.0, 1.0],
            },
        ],
    };
    let mut bindless_renderer = SceneRenderer::default();
    let bindless_image = scene_pixels(&mut bindless_renderer, &mut automatic, &meshes, &scene);
    assert_eq!(bindless_renderer.materials[0].unwrap().indices(), [0, 0]);
    assert_eq!(bindless_renderer.materials[1].unwrap().indices(), [1, 1]);
    let mut classic_renderer = SceneRenderer::default();
    let classic_image = scene_pixels(&mut classic_renderer, &mut classic, &meshes, &scene);
    let solid = pixel(&classic_image, 17, 28);
    let red_a = pixel(&classic_image, 42, 28);
    let red_b = pixel(&classic_image, 53, 28);
    assert!(solid[1] > 64 && solid[0] == 0, "{solid:?}");
    assert!(
        red_a[0] > 0 && red_b[0] > 0 && red_a[0] != red_b[0],
        "{red_a:?}, {red_b:?}"
    );
    assert!(
        bindless_image
            .iter()
            .zip(&classic_image)
            .all(|(bindless, classic)| bindless.abs_diff(*classic) <= 1),
        "classic and bindless images differ by more than one 8-bit color step"
    );
    eprintln!(
        "matched {SIDE}x{SIDE} frames with solid {solid:?} and checkerboard {red_a:?}, {red_b:?}"
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn metallic_and_roughness_change_direct_lighting() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let info = adapter.get_info();
    let mut table = ResourceTable::request(&adapter, true).unwrap();
    let mut meshes = MeshRegistry::default();
    let keys = [(0.0, 0.30), (0.0, 0.72), (1.0, 0.30), (1.0, 0.72)].map(|(metallic, roughness)| {
        meshes.insert(Mesh::sphere().with_pbr_material(PbrMaterial::new(metallic, roughness)))
    });
    let mut scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: vec![SceneObject {
            mesh: keys[0],
            transform: Mat4::from_translation(Vec3::new(0.0, 0.0, -2.5)),
            color: [0.55, 0.27, 0.09, 1.0],
        }],
    };
    let mut renderer = SceneRenderer {
        light: DirectionalLight::new(Vec3::Z, 0.30).unwrap(),
        ..Default::default()
    };
    let [
        smooth_dielectric,
        rough_dielectric,
        smooth_metal,
        rough_metal,
    ] = keys.map(|key| {
        scene.objects[0].mesh = key;
        center_pixel(&mut renderer, &mut table, &meshes, &scene)
    });
    assert!(smooth_dielectric[0] > rough_dielectric[0] + 8);
    assert!(smooth_metal[0] > rough_metal[0] + 16);
    assert!(rough_dielectric[0] > rough_metal[0] + 4);
    assert!(smooth_dielectric[2] > 12, "{smooth_dielectric:?}");
    assert!(smooth_metal[2] < smooth_metal[0], "{smooth_metal:?}");

    renderer.light = DirectionalLight::new(Vec3::NEG_Z, 0.30).unwrap();
    let backlit_metal = center_pixel(&mut renderer, &mut table, &meshes, &scene);
    assert!(rough_metal[0] > backlit_metal[0] + 4);
    eprintln!(
        "{} ({:?}): smooth/rough dielectric {smooth_dielectric:?} / {rough_dielectric:?}, \
         smooth/rough metal {smooth_metal:?} / {rough_metal:?}, backlit {backlit_metal:?}",
        info.name, info.backend,
    );
}

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
    pixel(
        &scene_pixels(renderer, table, meshes, scene),
        SIDE / 2,
        SIDE / 2,
    )
}

fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * SIDE + x) * 4) as usize;
    image[offset..offset + 4].try_into().unwrap()
}

fn scene_pixels(
    renderer: &mut SceneRenderer,
    table: &mut ResourceTable,
    meshes: &MeshRegistry,
    scene: &Scene,
) -> Vec<u8> {
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
    slice.get_mapped_range().unwrap().to_vec()
}
