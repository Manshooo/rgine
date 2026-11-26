#![cfg(feature = "authoring")]

use engine_core::{Reflect, TypeRegistry};
use engine_scene::{
    Change, CompileError, ObjectId, PrefabError, PrefabInstance, SceneTypeName, Truth,
    register_prefabs,
};
use serde::de::DeserializeOwned;
use uuid::Uuid;

const ASSET: Uuid = Uuid::from_u128(100);
const ROOT: ObjectId = ObjectId::from_u128(1);
const CHILD: ObjectId = ObjectId::from_u128(2);
const LEFT: ObjectId = ObjectId::from_u128(10);
const RIGHT: ObjectId = ObjectId::from_u128(20);
const SCENE_ROOT: ObjectId = ObjectId::from_u128(30);
const EXTRA: ObjectId = ObjectId::from_u128(40);

#[allow(dead_code)]
#[derive(Reflect)]
struct Node {
    asset: Uuid,
    name: String,
    scale: u32,
    target: Option<ObjectId>,
    owner: ObjectId,
}

fn registry() -> TypeRegistry {
    let mut registry = TypeRegistry::default();
    registry
        .register_reflect::<Node>()
        .insert_metadata(SceneTypeName("test.node"));
    register_prefabs(&mut registry);
    registry
}

fn source(registry: &TypeRegistry) -> Truth<'_> {
    let mut source = Truth::new(registry);
    source.create::<Node>(ROOT, None).unwrap();
    source.create::<Node>(CHILD, Some(ROOT)).unwrap();
    source
        .set_property(ROOT, "name", &"original".to_owned())
        .unwrap();
    source.set_property(ROOT, "target", &Some(CHILD)).unwrap();
    source.set_property(CHILD, "asset", &ASSET).unwrap();
    source.set_property(CHILD, "owner", &ROOT).unwrap();
    source.set_property(CHILD, "scale", &2_u32).unwrap();
    source
}

fn read<T: DeserializeOwned + 'static>(scene: &Truth<'_>, node: ObjectId, name: &str) -> T {
    scene
        .object(node)
        .unwrap()
        .property(name)
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn instances_reopen_and_compile_with_independent_ids_references_and_overrides() {
    let registry = registry();
    let mut source = source(&registry);
    let mut left = PrefabInstance::new(ASSET, ROOT);
    let right = PrefabInstance::new(ASSET, ROOT);
    let left_child = left.node_id(LEFT, CHILD);
    let right_child = right.node_id(RIGHT, CHILD);
    assert_ne!(left_child, right_child);
    left.set_override(&source, CHILD, "scale", &7_u32).unwrap();
    left.set_override(&source, CHILD, "target", &Some(SCENE_ROOT))
        .unwrap();

    let mut scene = Truth::new(&registry);
    scene.create::<Node>(SCENE_ROOT, None).unwrap();
    scene.create_prefab(LEFT, Some(SCENE_ROOT), left).unwrap();
    scene.create_prefab(RIGHT, Some(SCENE_ROOT), right).unwrap();
    scene.create::<Node>(EXTRA, Some(LEFT)).unwrap();
    scene
        .set_property(EXTRA, "target", &Some(right_child))
        .unwrap();
    assert!(matches!(
        scene.compile(|_, _, _| Ok::<_, ()>(())),
        Err(CompileError::UnresolvedPrefab(LEFT))
    ));

    let initial = scene
        .resolve_prefabs(|guid| (guid == ASSET).then_some(&source))
        .unwrap();
    assert_eq!(read::<u32>(&initial, left_child, "scale"), 7);
    assert_eq!(read::<u32>(&initial, right_child, "scale"), 2);
    assert_eq!(read::<String>(&initial, LEFT, "name"), "original");
    assert_eq!(read::<String>(&initial, RIGHT, "name"), "original");
    assert!(scene.object(left_child).is_none());

    let saved = scene.to_ron().unwrap();
    source
        .set_property(ROOT, "name", &"edited".to_owned())
        .unwrap();
    source.set_property(CHILD, "scale", &5_u32).unwrap();
    let source = Truth::from_ron(&registry, &source.to_ron().unwrap()).unwrap();
    let mut reopened = Truth::from_ron(&registry, &saved).unwrap();
    assert_eq!(reopened.to_ron().unwrap(), saved);
    let catalog = |guid| (guid == ASSET).then_some(&source);
    let final_scene = reopened.resolve_prefabs(catalog).unwrap();
    assert_eq!(read::<String>(&final_scene, LEFT, "name"), "edited");
    assert_eq!(read::<String>(&final_scene, RIGHT, "name"), "edited");
    assert_eq!(read::<u32>(&final_scene, left_child, "scale"), 7);
    assert_eq!(read::<u32>(&final_scene, right_child, "scale"), 5);
    assert_eq!(
        read::<Option<ObjectId>>(&final_scene, LEFT, "target"),
        Some(left_child)
    );
    assert_eq!(
        read::<Option<ObjectId>>(&final_scene, RIGHT, "target"),
        Some(right_child)
    );
    assert_eq!(read::<ObjectId>(&final_scene, right_child, "owner"), RIGHT);
    assert_eq!(read::<Uuid>(&final_scene, left_child, "asset"), ASSET);
    assert_eq!(
        read::<Option<ObjectId>>(&final_scene, left_child, "target"),
        Some(SCENE_ROOT)
    );
    assert_eq!(
        read::<Option<ObjectId>>(&final_scene, EXTRA, "target"),
        Some(right_child)
    );
    assert_eq!(final_scene.object(left_child).unwrap().parent(), Some(LEFT));
    assert!(
        final_scene
            .object(LEFT)
            .unwrap()
            .children()
            .any(|child| child == EXTRA)
    );

    let compiled = final_scene
        .compile(|_, node, references| {
            node.property("target")
                .and_then(|value| value.read::<Option<ObjectId>>().unwrap())
                .map(|id| references.get(id).ok_or("missing reference"))
                .transpose()
        })
        .unwrap()
        .into_compiled();
    let mut linked = 0;
    let world = compiled
        .instantiate(|world, _, target, entities| {
            if let Some(id) = target {
                let entity = entities.get(*id).ok_or("missing constituent entity")?;
                assert!(world.contains(entity));
                linked += 1;
            }
            Ok::<_, &str>(())
        })
        .unwrap();
    assert_eq!(world.entity_count(), 6);
    assert_eq!(linked, 4);

    let mut placement = reopened.prefab(LEFT).unwrap();
    placement.clear_override(CHILD, "scale");
    reopened.update_prefab(LEFT, &placement).unwrap();
    assert_eq!(
        read::<u32>(
            &reopened.resolve_prefabs(catalog).unwrap(),
            left_child,
            "scale"
        ),
        5
    );
    reopened.rewind(0).unwrap();
    assert_eq!(reopened.to_ron().unwrap(), saved);
}

#[test]
fn invalid_links_report_the_placement_and_keep_authored_state() {
    let registry = registry();
    let mut source = source(&registry);
    let mut scene = Truth::new(&registry);
    let mut instance = PrefabInstance::new(ASSET, ROOT);
    let child = instance.node_id(LEFT, CHILD);
    instance
        .set_override(&source, CHILD, "scale", &5_u32)
        .unwrap();
    scene.create_prefab(LEFT, None, instance).unwrap();
    assert!(
        matches!(scene.journal(), [entry] if matches!(entry.change, Change::PrefabCreated { .. }))
    );
    let saved = scene.to_ron().unwrap();

    assert!(matches!(
        scene.resolve_prefabs(|_| None),
        Err(PrefabError::MissingAsset {
            instance: LEFT,
            asset: ASSET
        })
    ));
    scene.create::<Node>(child, None).unwrap();
    assert!(
        matches!(scene.resolve_prefabs(|_| Some(&source)), Err(PrefabError::IdCollision { instance: LEFT, object }) if object == child)
    );
    scene.rewind(1).unwrap();
    source.delete(CHILD).unwrap();
    assert!(matches!(
        scene.resolve_prefabs(|_| Some(&source)),
        Err(PrefabError::MissingOverrideNode {
            instance: LEFT,
            node: CHILD
        })
    ));
    assert_eq!(scene.to_ron().unwrap(), saved);

    let mut placement = scene.prefab(LEFT).unwrap();
    placement.clear_override(CHILD, "scale");
    scene.update_prefab(LEFT, &placement).unwrap();
    assert!(matches!(
        scene.resolve_prefabs(|_| Some(&source)),
        Err(PrefabError::MissingTarget {
            instance: LEFT,
            node: ROOT,
            target: CHILD,
            ..
        })
    ));
    scene.rewind(0).unwrap();
    assert!(scene.object(LEFT).is_none());
}
