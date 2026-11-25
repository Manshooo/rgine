//! View the shipped level prop or supply a path to another static glTF or GLB.
use std::path::PathBuf;

use engine_app::{App, platform::WindowDesc, scene::*};
use engine_asset::{
    ImportCache,
    gltf_prop::{self, Prop},
    ktx2::Texture,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../asset/tests/fixtures/marker.gltf")
    });
    let cache = ImportCache::new(std::env::temp_dir().join("rgine-import-cache"));
    let artifact = gltf_prop::import(&path, &cache, &Default::default())?;
    let prop = Prop::load(&artifact)?;
    let mut registry = MeshRegistry::default();
    let objects = if let Some(texture_path) = args.next() {
        registry.register_textured_prop(&prop, Texture::load(&PathBuf::from(texture_path))?)?
    } else {
        registry.register_prop(&prop)?
    };
    let mut app = App::new().with_window(WindowDesc::new("rgine - glTF prop"));
    let world = app.world_mut();
    let camera = world.spawn_empty();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::default()).unwrap();
    for object in objects {
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
