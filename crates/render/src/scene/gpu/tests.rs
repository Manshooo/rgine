use std::collections::HashSet;
use std::sync::mpsc;
use std::time::Duration;

use engine_core::glam::{Mat4, Vec3};

use super::*;
use crate::canvas::{Affine2, Canvas, CanvasRenderer, Rect, Vec2, Viewport};
use crate::resources::ResourceError;
use crate::scene::SceneCamera;
use crate::shaders::SHADER_VARIANTS;

const SIDE: u32 = 64;

#[test]
fn validates_scene_pipeline_catalogue() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop.enable = true;
    validate_catalogue(descriptor);
}

#[test]
#[ignore = "requires a native GPU adapter with draw-uniform bindless support"]
fn validates_scene_pipeline_catalogue_on_native_backend() {
    validate_catalogue(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
}

fn validate_catalogue(descriptor: wgpu::InstanceDescriptor) {
    let instance = wgpu::Instance::new(descriptor);
    let mut covered = HashSet::new();
    for mode in [BindingMode::Classic, BindingMode::Bindless] {
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let mut table = ResourceTable::request(&adapter, mode == BindingMode::Classic).unwrap();
        assert_eq!(table.binding_mode(), mode);
        let mut renderer = SceneRenderer::default();
        for &format in SCENE_COLOR_FORMATS {
            renderer.skybox_enabled = false;
            renderer.prepare_pipelines(&mut table, format).unwrap();
            let keys = &renderer.pipelines.entries;
            assert!(keys.contains_key(&ScenePipelineKey::new(format, mode)));
            assert!(keys.contains_key(&ScenePipelineKey::DIRECTIONAL_SHADOW));
            assert!(!keys.contains_key(&ScenePipelineKey::skybox(format)));

            renderer.skybox_enabled = true;
            renderer.prepare_pipelines(&mut table, format).unwrap();
            assert!(
                renderer
                    .pipelines
                    .entries
                    .contains_key(&ScenePipelineKey::skybox(format))
            );
        }
        let prepared = renderer.pipeline_creations();
        for &format in SCENE_COLOR_FORMATS {
            renderer.prepare_pipelines(&mut table, format).unwrap();
            assert_eq!(renderer.pipeline_creations(), prepared);
        }
        println!(
            "{:?} {mode:?}: {prepared} scene pipelines prepared",
            adapter.get_info().backend
        );
        covered.extend(
            renderer
                .pipelines
                .entries
                .into_keys()
                .map(|key| (key.shader, key.color_format)),
        );
    }
    let declared: HashSet<_> = SHADER_VARIANTS
        .iter()
        .flat_map(|&variant| {
            let formats = if variant == ShaderVariantKey::DirectionalShadow {
                &[][..]
            } else {
                SCENE_COLOR_FORMATS
            };
            formats
                .iter()
                .copied()
                .map(Some)
                .chain(formats.is_empty().then_some(None))
                .map(move |format| (variant, format))
        })
        .collect();
    assert_eq!(
        covered, declared,
        "every declared shader and attachment configuration must be prepared"
    );
    println!("Validated {} distinct scene pipeline keys", covered.len());
}

#[test]
fn scene_pipeline_keys_include_binding_mode_and_layout() {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let classic = ScenePipelineKey::new(format, BindingMode::Classic);
    let bindless = ScenePipelineKey::new(format, BindingMode::Bindless);
    assert_ne!(classic, bindless);
    assert_ne!(classic.shader, bindless.shader);
    let sources = SceneShaderSources::default();
    assert!(classic.shader.wgsl(&sources).is_some());
    assert!(bindless.shader.wgsl(&sources).is_some());
    assert_ne!(classic.layout, bindless.layout);
    assert_eq!(
        (bindless.layout.sampled_textures, bindless.layout.samplers),
        (128, 16)
    );
}

#[cfg(feature = "shader-reload")]
#[test]
fn shader_reload_publishes_pipeline_replacements_together() {
    use std::{fs, path::Path};

    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop.enable = true;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let mut table = ResourceTable::request(&adapter, true).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut pipelines = ScenePipelines::default();
    pipelines.prepare(&mut table, format, true).unwrap();
    let previous = pipelines.entries.clone();

    let directory = tempfile::tempdir().unwrap();
    let scene_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/scene");
    for entry in scene_dir.read_dir().unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "wgsl")
        {
            fs::copy(&path, directory.path().join(path.file_name().unwrap())).unwrap();
        }
    }
    let object = directory.path().join("object.wgsl");
    let object_source = fs::read_to_string(&object).unwrap();
    fs::write(
        object,
        format!("{object_source}\n// Edit shared object source\n"),
    )
    .unwrap();
    let classic = directory.path().join("primitive_classic.wgsl");
    fs::write(&classic, "invalid wgsl").unwrap();
    let load = || {
        SceneShaderSources::load(directory.path(), &mut engine_asset::SourceFiles::default())
            .unwrap()
    };
    let error = pipelines.reload(&mut table, &load()).unwrap_err();
    assert!(error.to_string().contains("primitive_classic.wgsl"));
    assert_eq!(pipelines.entries, previous);
    assert_eq!(pipelines.created, previous.len());

    fs::copy(scene_dir.join("primitive_classic.wgsl"), classic).unwrap();
    let corrected = load();
    assert!(pipelines.reload(&mut table, &corrected).unwrap());
    assert_eq!(pipelines.entries.len(), previous.len());
    assert_eq!(pipelines.created, previous.len() + 2);
    let skybox = ScenePipelineKey::skybox(format);
    assert_eq!(pipelines.entries[&skybox], previous[&skybox]);
    assert!(!pipelines.reload(&mut table, &corrected).unwrap());
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
    assert_eq!(bindless_renderer.meshes[&solid].binding.indices(), [0, 0]);
    assert_eq!(bindless_renderer.meshes[&checker].binding.indices(), [1, 1]);
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
fn directional_shadow_darkens_the_receiver_and_follows_the_occluder() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let info = adapter.get_info();
    let mut meshes = MeshRegistry::default();
    let floor = meshes.insert(Mesh::plane().with_pbr_material(PbrMaterial::new(0.0, 0.85)));
    let cube = meshes.insert(Mesh::cube());
    let mut scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::look_at_rh(Vec3::Y * 6.0, Vec3::ZERO, Vec3::NEG_Z),
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: vec![
            SceneObject {
                mesh: floor,
                transform: Mat4::from_scale(Vec3::splat(5.0)),
                color: [0.7, 0.7, 0.7, 1.0],
            },
            SceneObject {
                mesh: cube,
                transform: Mat4::IDENTITY,
                color: [0.8, 0.0, 0.0, 1.0],
            },
        ],
    };
    for force_classic in [true, false] {
        let mut table = ResourceTable::request(&adapter, force_classic).unwrap();
        let mut renderer = SceneRenderer {
            light: DirectionalLight::new(Vec3::new(-1.0, 1.0, 0.0), 2.0).unwrap(),
            skybox_enabled: true,
            ..Default::default()
        };
        scene.objects[1].transform = Mat4::from_translation(Vec3::new(-1.0, 0.75, 0.0));
        let shadowed = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
        scene.objects[1].transform = Mat4::from_translation(Vec3::new(-1.0, 0.75, 1.5));
        let moved = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
        let shadow = pixel(&shadowed, SIDE / 2, SIDE / 2);
        let exposed = pixel(&moved, SIDE / 2, SIDE / 2);
        let unaffected = pixel(&shadowed, 48, SIDE / 2);
        assert!(
            shadow[0] > 30 && exposed[0] > shadow[0] + 40,
            "{shadow:?}, {exposed:?}"
        );
        assert_eq!(shadow[0], shadow[1], "the shadow lies on the gray floor");
        assert_eq!(unaffected, pixel(&moved, 48, SIDE / 2));
        assert!(unaffected[0] > shadow[0] + 40, "{unaffected:?}");
        eprintln!(
            "{} ({:?}, {:?}): shadowed floor {shadow:?}, after moving the occluder {exposed:?}, unshadowed floor {unaffected:?}",
            info.name,
            info.backend,
            table.binding_mode(),
        );
    }
}

#[test]
#[ignore = "requires a native GPU adapter with draw-uniform bindless support"]
fn dielectric_sphere_shows_floor_bounce_without_direct_light() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let mut meshes = MeshRegistry::default();
    let sphere = meshes.insert(Mesh::sphere().with_pbr_material(PbrMaterial::new(0.0, 0.7)));
    let scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: vec![SceneObject {
            mesh: sphere,
            transform: Mat4::from_translation(Vec3::new(0.0, 0.0, -2.5)),
            color: [0.8, 0.8, 0.8, 1.0],
        }],
    };
    for mode in [BindingMode::Classic, BindingMode::Bindless] {
        let mut table = ResourceTable::request(&adapter, mode == BindingMode::Classic).unwrap();
        assert_eq!(table.binding_mode(), mode);
        let mut renderer = SceneRenderer {
            light: DirectionalLight::new(Vec3::Y, 2.0).unwrap(),
            hemisphere_light: HemisphereLight::new(Vec3::splat(0.20), Vec3::new(0.07, 0.14, 0.28))
                .unwrap(),
            skybox_enabled: true,
            ..Default::default()
        };
        let lit = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
        renderer.light = DirectionalLight::new(Vec3::Y, 0.0).unwrap();
        let fill = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
        let underside = pixel(&lit, 32, 41);
        let top = pixel(&fill, 32, 22);
        assert_eq!(underside, pixel(&fill, 32, 41));
        assert!(underside[2] > underside[0] + 25, "{underside:?}");
        assert!(
            top[2] - top[0] < underside[2] - underside[0],
            "{top:?}, {underside:?}"
        );
        assert!(pixel(&lit, 32, 22)[0] > top[0] + 25);
        println!("{mode:?}: fill on the underside {underside:?}, fill on top {top:?}");
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn skybox_responds_to_rotation_and_remains_behind_opaque_geometry() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let info = adapter.get_info();
    let mut table = ResourceTable::request(&adapter, true).unwrap();
    let mut meshes = MeshRegistry::default();
    let cube = meshes.insert(Mesh::cube());
    let mut renderer = SceneRenderer {
        skybox_enabled: true,
        ..Default::default()
    };
    let camera = SceneCamera {
        view: Mat4::IDENTITY,
        vertical_fov: 1.0,
        near: 0.1,
        far: 20.0,
    };
    let mut scene = Scene {
        camera: Some(camera),
        objects: Vec::new(),
    };
    let initial = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
    let initial_center = pixel(&initial, SIDE / 2, SIDE / 2);
    assert!(initial_center[2] > 100, "{initial_center:?}");

    scene.camera.as_mut().unwrap().view = Mat4::from_translation(Vec3::new(-4.0, 2.0, -7.0));
    let translated = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
    assert!(initial == translated, "camera translation changed the sky");

    scene.camera.as_mut().unwrap().view = Mat4::from_rotation_x(0.65);
    let rotated = center_pixel(&mut renderer, &mut table, &meshes, &scene);
    assert!(
        initial_center[0].abs_diff(rotated[0]) > 25,
        "{initial_center:?}, {rotated:?}"
    );

    scene.camera = Some(camera);
    scene.objects.push(SceneObject {
        mesh: cube,
        transform: Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0)),
        color: [0.0, 1.0, 0.0, 1.0],
    });
    let composed = scene_pixels(&mut renderer, &mut table, &meshes, &scene);
    let foreground = pixel(&composed, SIDE / 2, SIDE / 2);
    assert!(foreground[1] > 64 && foreground[2] == 0, "{foreground:?}");
    assert_eq!(pixel(&composed, 4, 4), pixel(&initial, 4, 4));

    scene.camera = None;
    let no_camera = center_pixel(&mut renderer, &mut table, &meshes, &scene);
    assert_eq!(no_camera, [0, 0, 0, 255]);
    eprintln!(
        "{} ({:?}): sky {initial_center:?} before rotation and {rotated:?} after rotation, opaque foreground {foreground:?}",
        info.name, info.backend,
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

#[test]
#[ignore = "requires a native GPU adapter"]
fn imported_marker_renders_at_composed_node_transform() {
    use engine_asset::{
        ImportCache,
        gltf_prop::{self, Prop},
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../asset/tests/fixtures/marker.gltf");
    let dir = tempfile::tempdir().unwrap();
    let file =
        gltf_prop::import(&path, &ImportCache::new(dir.path()), &Default::default()).unwrap();
    let mut registry = MeshRegistry::default();
    let mut scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: registry.register_prop(&Prop::load(&file).unwrap()).unwrap(),
    };
    let gpu = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(gpu.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut table = ResourceTable::new(device, queue);
    let mut renderer = SceneRenderer::default();
    let center = center_pixel(&mut renderer, &mut table, &registry, &scene);
    assert!(
        center[0] > 32 && center[0] > center[1] && center[1] > center[2],
        "{center:?}"
    );
    scene.objects.clear();
    assert_ne!(
        center,
        center_pixel(&mut renderer, &mut table, &registry, &scene)
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn imported_ktx2_material_samples_mips_and_color_space() {
    use engine_asset::{ImportCache, gltf_prop, ktx2::Texture};
    use std::{fs, path::Path};

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../asset/tests/fixtures");
    let dir = tempfile::tempdir().unwrap();
    let file = gltf_prop::import(
        &fixtures.join("textured-quad.gltf"),
        &ImportCache::new(dir.path()),
        &Default::default(),
    )
    .unwrap();
    let prop = gltf_prop::Prop::load(&file).unwrap();
    let srgb_path = fixtures.join("mip-colors.ktx2");
    let linear_path = dir.path().join("linear.ktx2");
    let mut linear = fs::read(&srgb_path).unwrap();
    linear[12..16].copy_from_slice(&37u32.to_le_bytes());
    let dfd = u32::from_le_bytes(linear[48..52].try_into().unwrap()) as usize;
    linear[dfd + 14] = 1;
    linear[dfd + 79] = 0x0f;
    fs::write(&linear_path, linear).unwrap();

    let mut registry = MeshRegistry::default();
    let srgb = registry
        .register_textured_prop(&prop, Texture::load(&srgb_path).unwrap())
        .unwrap();
    let linear = registry
        .register_textured_prop(&prop, Texture::load(&linear_path).unwrap())
        .unwrap();
    let mut minified = srgb.clone();
    minified[0].transform *= Mat4::from_scale(Vec3::splat(0.08));
    let mut scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: Vec::new(),
    };
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let mut classic = None;
    for force_classic in [true, false] {
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let mut table = ResourceTable::request(&adapter, force_classic).unwrap();
        if !force_classic && table.binding_mode() == BindingMode::Classic {
            break;
        }
        let mut renderer = SceneRenderer {
            light: DirectionalLight::new(Vec3::Z, 0.0).unwrap(),
            hemisphere_light: HemisphereLight::new(Vec3::ONE, Vec3::ONE).unwrap(),
            ..Default::default()
        };
        let mut sample = |objects: &[SceneObject]| {
            scene.objects = objects.to_vec();
            center_pixel(&mut renderer, &mut table, &registry, &scene)
        };
        let pixels = [sample(&srgb), sample(&minified), sample(&linear)];
        let mode = table.binding_mode();
        eprintln!("{mode:?}: sRGB base, sRGB minified, linear base: {pixels:?}");
        for (actual, expected) in
            pixels
                .into_iter()
                .zip([[55u8, 0, 0, 255], [0, 55, 0, 255], [128, 0, 0, 255]])
        {
            assert!(
                actual
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 1)
            );
        }
        if let Some(previous) = classic.replace(pixels) {
            for (actual, expected) in pixels
                .into_iter()
                .flatten()
                .zip(previous.into_iter().flatten())
            {
                assert!(actual.abs_diff(expected) <= 1);
            }
        }
    }
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
    pixel_at_width(image, SIDE, x, y)
}

fn pixel_at_width(image: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * width + x) * 4) as usize;
    image[offset..offset + 4].try_into().unwrap()
}

fn scene_pixels(
    renderer: &mut SceneRenderer,
    table: &mut ResourceTable,
    meshes: &MeshRegistry,
    scene: &Scene,
) -> Vec<u8> {
    scene_pixels_at_size(renderer, table, meshes, scene, [SIDE, SIDE], None).0
}

fn scene_pixels_at_size(
    renderer: &mut SceneRenderer,
    table: &mut ResourceTable,
    meshes: &MeshRegistry,
    scene: &Scene,
    [width, height]: [u32; 2],
    overlay: Option<(&mut CanvasRenderer, &Canvas, Viewport)>,
) -> (Vec<u8>, usize) {
    let row = width * 4;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    renderer.prepare_pipelines(table, format).unwrap();
    let prepared = renderer.pipeline_creations();
    renderer
        .prepare(table, meshes, scene, [width, height])
        .unwrap();
    let size = wgpu::Extent3d {
        width,
        height,
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
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = table.device.create_command_encoder(&Default::default());
    let view = color.create_view(&Default::default());
    renderer
        .encode(
            table,
            scene,
            &mut encoder,
            &view,
            format,
            wgpu::Color::BLACK,
            None,
        )
        .unwrap();
    let draws = overlay.map_or(0, |(renderer, canvas, viewport)| {
        if renderer.prepare(table, canvas, viewport, format).unwrap() {
            renderer
                .encode(table, &mut encoder, &view, format, None)
                .unwrap()
        } else {
            0
        }
    });
    assert_eq!(renderer.pipeline_creations() - prepared, 0);
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
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
    (slice.get_mapped_range().unwrap().to_vec(), draws)
}

#[test]
#[ignore = "requires a native GPU adapter for offscreen canvas pixels"]
fn canvas_batches_preserve_order_clipping_and_scale_over_a_3d_scene() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let mut table = ResourceTable::request(&adapter, true).unwrap();
    let mut meshes = MeshRegistry::default();
    let cube = meshes.insert(Mesh::cube());
    let scene = Scene {
        camera: Some(SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        }),
        objects: vec![SceneObject {
            mesh: cube,
            transform: Mat4::from_translation(Vec3::NEG_Z * 3.0),
            color: [0.0, 1.0, 0.0, 1.0],
        }],
    };
    let a = canvas_image(&mut table, &[255, 255, 0, 255, 0, 255, 255, 255]);
    let b = canvas_image(&mut table, &[255, 0, 255, 255]);
    let rect = |x, y, w, h| Rect::from_min_size(Vec2::new(x, y), Vec2::new(w, h));
    let identity = Affine2::IDENTITY;
    let uv = rect(0.0, 0.0, 1.0, 1.0);
    let white = [1.0; 4];
    let mut canvas = Canvas::default();
    for _ in 0..200 {
        canvas.solid_quad(rect(1.0, 1.0, 2.0, 2.0), identity, white);
    }
    canvas.solid_quad(rect(16.0, 14.0, 32.0, 30.0), identity, [1.0, 0.0, 0.0, 0.5]);
    canvas.solid_quad(rect(26.0, 20.0, 20.0, 17.0), identity, [0.0, 0.0, 1.0, 0.5]);
    let rotated =
        Affine2::from_angle_translation(std::f32::consts::FRAC_PI_2, Vec2::new(21.0, 4.0));
    canvas.textured_quad(rect(0.0, 0.0, 10.0, 6.0), uv, rotated, white, a);
    let overlap = rect(56.0, 20.0, 7.0, 6.0);
    canvas.textured_quad(overlap, uv, identity, white, a);
    canvas.textured_quad(overlap, uv, identity, white, b);
    canvas.textured_quad(
        rect(58.0, 20.0, 4.0, 6.0),
        rect(0.0, 0.0, 0.49, 1.0),
        identity,
        [1.0, 1.0, 1.0, 0.5],
        a,
    );
    canvas.push_clip(rect(42.0, 4.0, 12.0, 12.0));
    canvas.push_clip(rect(47.0, 1.0, 12.0, 12.0));
    canvas.solid_quad(rect(40.0, 0.0, 20.0, 20.0), identity, [0.0, 1.0, 0.0, 1.0]);
    assert!(canvas.pop_clip());
    assert!(canvas.pop_clip());
    canvas.line(
        Vec2::new(2.0, 44.0),
        Vec2::new(12.0, 44.0),
        2.0,
        identity,
        white,
    );

    let mut scene_renderer = SceneRenderer::default();
    let mut canvas_renderer = CanvasRenderer::default();
    for (size, scale) in [([64, 64], 1), ([128, 96], 2)] {
        let viewport = Viewport::new(size, scale as f32).unwrap();
        assert_eq!(
            viewport.logical_size(),
            Vec2::new(64.0, (size[1] / scale) as f32)
        );
        let (base, _) =
            scene_pixels_at_size(&mut scene_renderer, &mut table, &meshes, &scene, size, None);
        let (image, draws) = scene_pixels_at_size(
            &mut scene_renderer,
            &mut table,
            &meshes,
            &scene,
            size,
            Some((&mut canvas_renderer, &canvas, viewport)),
        );
        assert_eq!(draws, 6);
        let sample = |pixels: &[u8], x, y| pixel_at_width(pixels, size[0], x * scale, y * scale);
        let centre_y = size[1] / scale / 2;
        let under = sample(&base, 32, centre_y);
        assert!(under[1] > 32, "3D cube pixel {under:?}");
        let composed = sample(&image, 32, centre_y);
        for (actual, expected) in composed.into_iter().zip([64, under[1] / 4, 128, 255]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "ordered alpha composite {composed:?}"
            );
        }
        assert_eq!(sample(&image, 18, 6), [255, 255, 0, 255]);
        assert_eq!(sample(&image, 18, 11), [0, 255, 255, 255]);
        assert_eq!(sample(&image, 57, 23), [255, 0, 255, 255]);
        let texture_mix = sample(&image, 59, 23);
        for (actual, expected) in texture_mix.into_iter().zip([255, 128, 128, 255]) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "texture order {texture_mix:?}"
            );
        }
        assert_eq!(sample(&image, 47, 6), [0, 255, 0, 255]);
        for (x, y) in [(46, 6), (54, 6), (48, 3), (48, 13), (7, 42)] {
            assert_eq!(
                sample(&image, x, y),
                sample(&base, x, y),
                "outside at {x},{y}"
            );
        }
        assert_eq!(sample(&image, 7, 44), [255; 4]);
    }
}

fn canvas_image(table: &mut ResourceTable, texels: &[u8]) -> SampledGroupHandle {
    let texture = table
        .create_texture(TextureDescriptor::sampled_rgba8([
            (texels.len() / 4) as u32,
            1,
        ]))
        .unwrap();
    table.write_texture(texture, 0, 0, texels).unwrap();
    let sampler = table
        .create_sampler(SamplerDescriptor::NEAREST_CLAMP)
        .unwrap();
    table
        .create_sampled_group(SampledBinding { texture, sampler })
        .unwrap()
}
