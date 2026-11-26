#![cfg(feature = "authoring")]

use engine_core::{Reflect, TypeRegistry};
use engine_scene::{ObjectId, SceneError, SceneTypeName, Truth, TruthError, migrate_scene_ron};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const SCENE: &str = include_str!("fixtures/authored.scene.ron");
const SCENE_V1: &str = include_str!("fixtures/authored-v1.scene.ron");
const CHILD: ObjectId = ObjectId::from_u128(1);
const ROOT: ObjectId = ObjectId::from_u128(2);
const ASSET: Uuid = Uuid::from_u128(100);

#[allow(dead_code)]
#[derive(Reflect)]
#[reflect(version = 2)]
struct Node {
    asset: Uuid,
    mode: Mode,
    name: String,
    target: Option<ObjectId>,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
enum Mode {
    Animated { speed: u32 },
}

fn register_node(registry: &mut TypeRegistry) {
    registry
        .register_reflect::<Node>()
        .insert_metadata(SceneTypeName("test.node"));
}

#[test]
fn scene_round_trips_hierarchy_and_references_across_registries() {
    let mut writing_registry = TypeRegistry::default();
    register_node(&mut writing_registry);
    let mut truth = Truth::new(&writing_registry);
    truth.create::<Node>(ROOT, None).unwrap();
    truth.create::<Node>(CHILD, Some(ROOT)).unwrap();
    truth
        .set_property(CHILD, "name", &"child".to_owned())
        .unwrap();
    truth.set_property(CHILD, "asset", &ASSET).unwrap();
    truth
        .set_property(CHILD, "mode", &Mode::Animated { speed: 2 })
        .unwrap();
    truth.set_property(ROOT, "target", &Some(CHILD)).unwrap();
    let saved = truth.to_ron().unwrap();
    assert_eq!(saved, SCENE);
    assert_eq!(truth.to_ron().unwrap(), saved);

    let mut reading_registry = TypeRegistry::default();
    reading_registry.register::<u32>();
    register_node(&mut reading_registry);
    let mut loaded = Truth::from_ron(&reading_registry, SCENE).unwrap();
    assert!(loaded.journal().is_empty());
    assert_eq!(
        loaded.objects().map(|(id, _)| id).collect::<Vec<_>>(),
        [CHILD, ROOT]
    );
    let child = loaded.object(CHILD).unwrap();
    let root = loaded.object(ROOT).unwrap();
    assert_ne!(child.type_key(), truth.object(CHILD).unwrap().type_key());
    assert_eq!(
        child.type_key(),
        reading_registry.get::<Node>().unwrap().key()
    );
    assert_eq!(child.parent(), Some(ROOT));
    assert_eq!(root.children().collect::<Vec<_>>(), [CHILD]);
    assert_eq!(
        child.property("asset").unwrap().read::<Uuid>().unwrap(),
        ASSET
    );
    assert_eq!(
        child.property("mode").unwrap().read::<Mode>().unwrap(),
        Mode::Animated { speed: 2 }
    );
    assert_eq!(
        root.property("target")
            .unwrap()
            .read::<Option<ObjectId>>()
            .unwrap(),
        Some(CHILD)
    );
    assert_eq!(loaded.to_ron().unwrap(), saved);

    loaded.delete(CHILD).unwrap();
    let deleted = loaded.to_ron().unwrap();
    let reopened = Truth::from_ron(&reading_registry, &deleted).unwrap();
    assert!(reopened.object(CHILD).is_none());
    assert_eq!(
        reopened
            .object(ROOT)
            .unwrap()
            .property("target")
            .unwrap()
            .read::<Option<ObjectId>>()
            .unwrap(),
        Some(CHILD)
    );
    loaded.rewind(0).unwrap();
    assert_eq!(loaded.to_ron().unwrap(), saved);
}

#[test]
fn previous_scene_schema_migrates_and_opens_with_the_current_loader() {
    let mut registry = TypeRegistry::default();
    register_node(&mut registry);
    let migrated = migrate_scene_ron(SCENE_V1).unwrap().unwrap();
    assert_eq!(migrated, SCENE);
    assert_eq!(migrate_scene_ron(&migrated).unwrap(), None);
    let previous = Truth::from_ron(&registry, SCENE_V1).unwrap();
    let current = Truth::from_ron(&registry, &migrated).unwrap();
    assert_eq!(
        previous.objects().collect::<Vec<_>>(),
        current.objects().collect::<Vec<_>>()
    );
    assert_eq!(current.to_ron().unwrap(), SCENE);
}

#[test]
fn unsupported_versions_are_reported() {
    let mut registry = TypeRegistry::default();
    register_node(&mut registry);
    let future = "(schema_version: 42, new_layout: [NewNode(name: \"future\")])";
    assert!(matches!(
        Truth::from_ron(&registry, future),
        Err(SceneError::UnsupportedVersion(42))
    ));

    let wrong_type_version = SCENE.replacen(
        "\n                version: 2",
        "\n                version: 3",
        1,
    );
    assert!(matches!(
        Truth::from_ron(&registry, &wrong_type_version),
        Err(SceneError::UnsupportedTypeVersion {
            object: CHILD,
            found: 3,
            expected: 2,
            ..
        })
    ));
}

#[test]
fn invalid_document_edges_and_keys_are_rejected() {
    let mut registry = TypeRegistry::default();
    register_node(&mut registry);
    let missing_parent = SCENE.replace(&format!("Some(\"{ROOT}\")"), &format!("Some(\"{ASSET}\")"));
    assert!(matches!(
        Truth::from_ron(&registry, &missing_parent),
        Err(SceneError::MissingParent { object: CHILD, .. })
    ));
    let cycle = SCENE.replace("parent: None", &format!("parent: Some(\"{CHILD}\")"));
    assert!(matches!(
        Truth::from_ron(&registry, &cycle),
        Err(SceneError::CyclicHierarchy(CHILD))
    ));
    let duplicate_id = SCENE.replace(&format!("\"{ROOT}\": ("), &format!("\"{CHILD}\": ("));
    let duplicate_property = SCENE.replace(
        "\"name\": \"child\",",
        "\"name\": \"child\", \"name\": \"second\",",
    );
    for duplicate in [duplicate_id, duplicate_property] {
        let error = Truth::from_ron(&registry, &duplicate).unwrap_err();
        assert!(error.to_string().contains("duplicate key"), "{error}");
    }
    let unknown_field = SCENE.replace("\"name\": \"child\"", "\"unknown\": \"child\"");
    assert!(matches!(
        Truth::from_ron(&registry, &unknown_field),
        Err(SceneError::InvalidObject(TruthError::UnknownProperty {
            object: CHILD,
            ..
        }))
    ));
}
