//! Edit the RON scene or the glTF source files while the game fixture is running.

use std::path::PathBuf;

use engine_app::{
    App,
    core::TypeRegistry,
    hot_reload::{PropScene, register_prop_nodes},
    platform::WindowDesc,
    scene::{Camera, MeshRegistry, Transform},
};
use engine_asset::ImportCache;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let scene = arguments.next().map(PathBuf::from);
    let model = arguments.next().map(PathBuf::from);
    let (Some(scene), Some(model)) = (scene, model) else {
        return Err("usage: hot_reload <scene.ron> <model.gltf> [base-color.ktx2]".into());
    };
    let texture = arguments.next().map(PathBuf::from);
    let mut registry = TypeRegistry::default();
    register_prop_nodes(&mut registry);
    let cache = ImportCache::new(std::env::temp_dir().join("rgine-import-cache"));
    let mut loader = PropScene::new(&registry, scene, model, texture, cache);

    let mut app = App::new().with_window(WindowDesc::new("rgine - hot reload").with_size(320, 240));
    let world = app.world_mut();
    let camera = world.spawn_empty();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::default()).unwrap();
    let mut meshes = MeshRegistry::default();
    loader.load(world, &mut meshes)?;
    app.run_scene_with_refresh(meshes, [0.02, 0.03, 0.04, 1.0], |world, meshes| {
        loader.poll(world, meshes)
    })?;
    Ok(())
}
