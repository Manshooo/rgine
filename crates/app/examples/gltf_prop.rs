//! View the shipped level prop or supply a path to another static glTF or GLB.
use std::path::PathBuf;

use engine_app::{App, platform::WindowDesc, scene::*};
use engine_asset::{
    ImportCache,
    gltf_prop::{self, Prop},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../asset/tests/fixtures/marker.gltf")
        });
    let cache = ImportCache::new(std::env::temp_dir().join("rgine-import-cache"));
    let artifact = gltf_prop::import(&path, &cache, &Default::default())?;
    let mut registry = MeshRegistry::default();
    let mut app = App::new().with_window(WindowDesc::new("rgine - glTF prop"));
    let world = app.world_mut();
    let camera = world.spawn_empty();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::default()).unwrap();
    for object in registry.register_prop(&Prop::load(&artifact)?)? {
        let entity = world.spawn_empty();
        world.insert(entity, Transform::default()).unwrap();
        world.insert(entity, NodeMatrix(object.transform)).unwrap();
        world
            .insert(
                entity,
                Renderable {
                    mesh: object.mesh,
                    color: object.color,
                },
            )
            .unwrap();
    }
    Ok(app.run_scene(registry, [0.025, 0.035, 0.06, 1.0])?)
}
