//! Development-only reload for a static prop and its authored placements.

use std::{
    convert::Infallible,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use engine_asset::{
    ImportCache, SourceFiles,
    gltf_prop::{self, Prop},
    ktx2::Texture,
};
use engine_core::{Reflect, TypeRegistry};
use engine_ecs::{Component, World};
use engine_render::scene::SceneObject;
use engine_scene::{SceneCompiler, SceneTypeName, Truth};

use crate::scene::{MeshRegistry, Transform};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Reflect)]
struct PropNode {
    #[allow(dead_code)]
    translation: [f32; 3],
}

#[derive(Debug, Component)]
pub(crate) struct PropInstances(pub(crate) Arc<[SceneObject]>);

/// Registers the authored placement type rgine.static_prop.
pub fn register_prop_nodes(registry: &mut TypeRegistry) {
    registry
        .register_reflect::<PropNode>()
        .insert_metadata(SceneTypeName("rgine.static_prop"));
}

/// Watches one static prop, its external buffers and optional base-color map,
/// and the RON scene that places copies of that prop.
///
/// The same instance refreshes one application world and its mesh registry.
pub struct PropScene<'registry> {
    registry: &'registry TypeRegistry,
    compiler: SceneCompiler<'registry>,
    scene: PathBuf,
    model: PathBuf,
    texture: Option<PathBuf>,
    cache: ImportCache,
    asset_inputs: SourceFiles,
    scene_inputs: SourceFiles,
    asset_pending: bool,
    active_objects: Option<Arc<[SceneObject]>>,
    last_poll: Instant,
}

impl<'registry> PropScene<'registry> {
    /// Creates the watcher. Register the placement schema before loading the scene.
    pub fn new(
        registry: &'registry TypeRegistry,
        scene: PathBuf,
        model: PathBuf,
        texture: Option<PathBuf>,
        cache: ImportCache,
    ) -> Self {
        Self {
            registry,
            compiler: SceneCompiler::new(registry),
            scene,
            model,
            texture,
            cache,
            asset_inputs: SourceFiles::default(),
            scene_inputs: SourceFiles::default(),
            asset_pending: true,
            active_objects: None,
            last_poll: Instant::now(),
        }
    }

    /// Loads the first valid scene before the game loop starts.
    pub fn load(&mut self, world: &mut World, meshes: &mut MeshRegistry) -> io::Result<()> {
        self.last_poll = Instant::now();
        self.reload(world, meshes)
    }

    /// Polls the tracked inputs and commits a complete change between frames.
    /// A failed attempt reports its source path and leaves the current scene active.
    pub fn poll(&mut self, world: &mut World, meshes: &mut MeshRegistry) -> bool {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return false;
        }
        self.last_poll = Instant::now();
        let asset_changed = self.asset_inputs.changed();
        if !asset_changed && !self.scene_inputs.changed() {
            return false;
        }
        self.asset_pending |= asset_changed;
        match self.reload(world, meshes) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("hot reload: {error}");
                false
            }
        }
    }

    fn reload(&mut self, world: &mut World, meshes: &mut MeshRegistry) -> io::Result<()> {
        // Record both attempts before propagating an error. A fix to either input
        // can retry an asset that was staged while the scene was invalid.
        let assets = self.asset_pending.then(|| self.read_assets());
        let scene = self.read_scene();
        let assets = assets.transpose()?;
        let truth = scene?;
        let objects = assets
            .as_ref()
            .map(|(_, objects)| objects)
            .or(self.active_objects.as_ref())
            .expect("initial reload stages assets");
        let before = world.tick();
        self.compiler
            .update(
                world,
                &truth,
                |_, node, _| {
                    let mut translation = [0.0_f32; 3];
                    if let Some(value) = node.property("translation") {
                        translation = value.read().map_err(io::Error::other)?;
                    }
                    if translation.iter().any(|value| !value.is_finite()) {
                        return Err(io::Error::other("expected a finite translation"));
                    }
                    Ok(translation)
                },
                |commands, translation, _| {
                    commands.insert(Transform {
                        translation: (*translation).into(),
                        ..Transform::default()
                    });
                    commands.insert(PropInstances(objects.clone()));
                    Ok::<_, Infallible>(())
                },
            )
            .map_err(|error| source_error(&self.scene, error))?;

        if let Some((registry, objects)) = assets {
            if world.tick() == before {
                world.advance_tick();
            }
            for (id, _) in truth.objects() {
                let entity = self.compiler.entity(world, id).expect("compiled placement");
                if let Some(instance) = world.get_mut::<PropInstances>(entity) {
                    instance.0 = objects.clone();
                }
            }
            *meshes = registry;
            self.active_objects = Some(objects);
            self.asset_pending = false;
        }
        Ok(())
    }

    fn read_assets(&mut self) -> io::Result<(MeshRegistry, Arc<[SceneObject]>)> {
        self.asset_inputs = SourceFiles::default();
        let artifact = gltf_prop::import_tracked(
            &self.model,
            &self.cache,
            &Default::default(),
            &mut self.asset_inputs,
        )
        .map_err(|error| source_error(&self.model, error))?;
        let prop = Prop::load(&artifact).map_err(|error| source_error(&self.model, error))?;
        let mut meshes = MeshRegistry::default();
        let objects = match &self.texture {
            Some(path) => {
                let texture = Texture::load_tracked(path, &mut self.asset_inputs)?;
                meshes.register_textured_prop(&prop, texture)
            }
            None => meshes.register_prop(&prop),
        }
        .map_err(|error| source_error(&self.model, error))?;
        Ok((meshes, objects.into()))
    }

    fn read_scene(&mut self) -> io::Result<Truth<'registry>> {
        self.scene_inputs = SourceFiles::default();
        let bytes = self
            .scene_inputs
            .read(&self.scene)
            .map_err(|error| source_error(&self.scene, error))?;
        let text = std::str::from_utf8(&bytes).map_err(|error| source_error(&self.scene, error))?;
        Truth::from_ron(self.registry, text).map_err(|error| source_error(&self.scene, error))
    }
}

fn source_error(path: &Path, error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod phase3;

#[cfg(test)]
mod tests {
    use std::fs;

    use engine_scene::ObjectId;

    use super::*;

    #[test]
    fn a_rejected_scene_keeps_the_active_asset_and_retries_it_on_correction() {
        let temp = tempfile::tempdir().unwrap();
        let model = temp.path().join("marker.gltf");
        let buffer = temp.path().join("marker.bin");
        let scene = temp.path().join("level.scene.ron");
        let authored = include_str!("../examples/fixtures/hot-reload.scene.ron");
        fs::write(
            &model,
            include_bytes!("../../asset/tests/fixtures/marker.gltf"),
        )
        .unwrap();
        let mut geometry = *include_bytes!("../../asset/tests/fixtures/marker.bin");
        fs::write(&buffer, geometry).unwrap();
        fs::write(&scene, authored).unwrap();

        let mut registry = TypeRegistry::default();
        register_prop_nodes(&mut registry);
        let mut loader = PropScene::new(
            &registry,
            scene.clone(),
            model,
            None,
            ImportCache::new(temp.path().join("cache")),
        );
        let mut world = World::new();
        let mut meshes = MeshRegistry::default();
        loader.load(&mut world, &mut meshes).unwrap();
        let before = crate::scene::extract(&world);
        let placement = ObjectId::from_u128(1);
        let entity = loader.compiler.entity(&world, placement).unwrap();

        geometry[..4].copy_from_slice(&(-0.5_f32).to_le_bytes());
        fs::write(&buffer, geometry).unwrap();
        fs::write(&scene, "(").unwrap();
        loader.last_poll = Instant::now() - POLL_INTERVAL;
        assert!(!loader.poll(&mut world, &mut meshes));
        assert_eq!(crate::scene::extract(&world), before);

        fs::write(
            &scene,
            authored.replace("(0.0, 0.0, 0.0)", "(1.0, 0.0, 0.0)"),
        )
        .unwrap();
        loader.last_poll = Instant::now() - POLL_INTERVAL;
        assert!(loader.poll(&mut world, &mut meshes));
        let after = crate::scene::extract(&world);
        assert_ne!(after.objects[0].mesh, before.objects[0].mesh);
        assert_eq!(
            after.objects[0].transform.w_axis.x,
            before.objects[0].transform.w_axis.x + 1.0
        );
        assert_eq!(loader.compiler.entity(&world, placement), Some(entity));
    }
}
