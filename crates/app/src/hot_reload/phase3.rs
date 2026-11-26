use std::{fs, thread};

use engine_asset::AssetGuid;
use engine_ecs::ChildOf;
use engine_scene::{ObjectId, PrefabInstance, merge_scene_ron, register_prefabs};

use super::*;

#[test]
fn assets_and_scenes_phase_exit() {
    const ASSET: AssetGuid = AssetGuid::from_u128(100);
    const ROOT: ObjectId = ObjectId::from_u128(1);
    const CHILD: ObjectId = ObjectId::from_u128(2);
    const INSTANCE: ObjectId = ObjectId::from_u128(10);
    const LOOSE: ObjectId = ObjectId::from_u128(20);

    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../asset/tests/fixtures");
    let model = temp.path().join("textured-quad.gltf");
    let texture = temp.path().join("mip-colors.ktx2");
    let prefab_path = temp.path().join("quad.prefab.ron");
    let authored_path = temp.path().join("level.scene.ron");
    let live_path = temp.path().join("resolved.scene.ron");
    fs::copy(fixtures.join("textured-quad.gltf"), &model).unwrap();
    fs::copy(fixtures.join("mip-colors.ktx2"), &texture).unwrap();

    let mut registry = TypeRegistry::default();
    register_prop_nodes(&mut registry);
    register_prefabs(&mut registry);
    let mut prefab = Truth::new(&registry);
    prefab.create::<PropNode>(ROOT, None).unwrap();
    prefab.create::<PropNode>(CHILD, Some(ROOT)).unwrap();
    prefab
        .set_property(CHILD, "translation", &[0.0_f32, 1.0, 0.0])
        .unwrap();
    fs::write(&prefab_path, prefab.to_ron().unwrap()).unwrap();
    let prefab = Truth::from_ron(&registry, &fs::read_to_string(prefab_path).unwrap()).unwrap();

    let instance = PrefabInstance::new(ASSET, ROOT);
    let child = instance.node_id(INSTANCE, CHILD);
    let mut authored = Truth::new(&registry);
    authored
        .create_prefab(INSTANCE, None, instance.clone())
        .unwrap();
    authored.create::<PropNode>(LOOSE, None).unwrap();
    authored
        .set_property(LOOSE, "translation", &[0.0_f32, -1.0, 0.0])
        .unwrap();
    let base = authored.to_ron().unwrap();
    fs::write(&authored_path, &base).unwrap();
    let open = || Truth::from_ron(&registry, &fs::read_to_string(&authored_path).unwrap()).unwrap();
    let resolve = |scene: &Truth<'_>| {
        let resolved = scene
            .resolve_prefabs(|guid| (guid == ASSET).then_some(&prefab))
            .unwrap();
        fs::write(&live_path, resolved.to_ron().unwrap()).unwrap();
    };
    let reopened = open();
    assert_eq!(reopened.to_ron().unwrap(), base);
    assert_eq!(reopened.prefab(INSTANCE).unwrap(), instance);
    resolve(&reopened);

    let mut loader = PropScene::new(
        &registry,
        live_path.clone(),
        model.clone(),
        Some(texture),
        ImportCache::new(temp.path().join("cache")),
    );
    let mut world = World::new();
    let mut meshes = MeshRegistry::default();
    loader.load(&mut world, &mut meshes).unwrap();
    let ids = [INSTANCE, child, LOOSE];
    let entities = ids.map(|id| loader.compiler.entity(&world, id).unwrap());
    let initial = crate::scene::extract(&world);
    assert_eq!(initial.objects.len(), 3);
    assert!(
        initial
            .objects
            .iter()
            .all(|object| object.transform.w_axis.z == -2.0)
    );

    let mut gltf: serde_json::Value = serde_json::from_slice(&fs::read(&model).unwrap()).unwrap();
    gltf["nodes"][0]["translation"][2] = (-3).into();
    fs::write(&model, serde_json::to_vec(&gltf).unwrap()).unwrap();
    wait_for_reload(&mut loader, &mut world, &mut meshes);
    let reloaded = crate::scene::extract(&world);
    assert_eq!(reloaded.objects.len(), 3);
    assert!(
        reloaded
            .objects
            .iter()
            .all(|object| object.transform.w_axis.z == -3.0)
    );
    assert_ne!(reloaded.objects[0].mesh, initial.objects[0].mesh);

    let mut ours = open();
    let mut edited = ours.prefab(INSTANCE).unwrap();
    edited
        .set_override(&prefab, CHILD, "translation", &[0.0_f32, 2.0, 0.0])
        .unwrap();
    ours.update_prefab(INSTANCE, &edited).unwrap();
    let mut theirs = open();
    theirs
        .set_property(LOOSE, "translation", &[0.0_f32, -2.0, 0.0])
        .unwrap();
    let merged =
        merge_scene_ron(&base, &ours.to_ron().unwrap(), &theirs.to_ron().unwrap()).unwrap();
    fs::write(&authored_path, merged).unwrap();
    resolve(&open());
    wait_for_reload(&mut loader, &mut world, &mut meshes);

    assert_eq!(
        ids.map(|id| loader.compiler.entity(&world, id).unwrap()),
        entities
    );
    assert_eq!(
        entities.map(|entity| world.get::<Transform>(entity).unwrap().translation.y),
        [0.0, 2.0, -2.0],
    );
    assert!(
        world
            .relation_targets::<ChildOf>(entities[1])
            .eq([entities[0]])
    );
    let final_scene = crate::scene::extract(&world);
    let mut positions: Vec<_> = final_scene
        .objects
        .iter()
        .map(|object| {
            assert_eq!(object.mesh, reloaded.objects[0].mesh);
            assert_eq!(object.transform.w_axis.z, -3.0);
            object.transform.w_axis.y
        })
        .collect();
    positions.sort_by(f32::total_cmp);
    assert_eq!(positions, [-2.0, 0.0, 2.0]);
}

fn wait_for_reload(loader: &mut PropScene<'_>, world: &mut World, meshes: &mut MeshRegistry) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !loader.poll(world, meshes) {
        assert!(
            Instant::now() < deadline,
            "edited files never reached the live world"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
