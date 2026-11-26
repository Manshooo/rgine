#![cfg(feature = "authoring")]

use std::sync::Mutex;

use engine_core::{Reflect, TypeRegistry};
use engine_scene::{Change, ObjectId, Truth, TruthError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Reflect)]
#[reflect(version = 2)]
struct Node {
    name: String,
    target: Option<ObjectId>,
    asset: Uuid,
    mode: Mode,
    notes: Mutex<Vec<String>>,
}

#[derive(Debug, Reflect, Serialize, Deserialize, PartialEq, Eq)]
enum Mode {
    Animated { speed: u32 },
}

#[derive(Reflect)]
struct Unregistered;

fn id(value: u128) -> ObjectId {
    ObjectId::from_u128(value)
}

#[test]
fn mixed_edits_rewind_identity_values_hierarchy_and_references() {
    let mut registry = TypeRegistry::default();
    let node_type = registry.register_reflect::<Node>().key();
    let mut truth = Truth::new(&registry);
    let [root, other, child, leaf, extra] = [1, 2, 3, 4, 5].map(id);
    let asset = Uuid::from_u128(100);
    truth.create::<Node>(root, None).unwrap();
    truth.create::<Node>(other, None).unwrap();
    truth.create::<Node>(child, Some(root)).unwrap();
    truth.create::<Node>(leaf, Some(child)).unwrap();
    truth.set_property(root, "target", &Some(leaf)).unwrap();
    truth.set_property(child, "target", &Some(root)).unwrap();
    truth
        .set_property(child, "name", &"original".to_owned())
        .unwrap();
    truth.set_property(leaf, "asset", &asset).unwrap();
    truth
        .set_property(leaf, "mode", &Mode::Animated { speed: 2 })
        .unwrap();
    let checkpoint = truth.journal().len();

    truth.reparent(child, Some(other)).unwrap();
    assert_eq!(
        truth.journal().last().unwrap().change,
        Change::Reparented {
            old: Some(root),
            new: Some(other)
        }
    );
    truth
        .set_property(child, "name", &"edited".to_owned())
        .unwrap();
    let Change::Property {
        name,
        old: Some(old),
        new: Some(new),
    } = &truth.journal().last().unwrap().change
    else {
        panic!("expected a property edit");
    };
    assert_eq!(*name, "name");
    assert_eq!(old.read::<String>().unwrap(), "original");
    assert_eq!(new.read::<String>().unwrap(), "edited");
    truth.clear_property(child, "target").unwrap();
    truth.create::<Node>(extra, Some(child)).unwrap();
    truth.delete(child).unwrap();
    let Change::Deleted { old } = &truth.journal().last().unwrap().change else {
        panic!("expected a subtree deletion");
    };
    assert_eq!(
        old.iter().map(|&(id, _)| id).collect::<Vec<_>>(),
        [child, leaf, extra]
    );
    assert_eq!(truth.object(other).unwrap().children().len(), 0);
    assert!(truth.object(leaf).is_none());
    assert_eq!(
        truth
            .object(root)
            .unwrap()
            .property("target")
            .unwrap()
            .read::<Option<ObjectId>>()
            .unwrap(),
        Some(leaf)
    );

    truth.create::<Node>(leaf, None).unwrap();
    truth.rewind(checkpoint).unwrap();
    assert_eq!(
        truth.objects().map(|(id, _)| id).collect::<Vec<_>>(),
        [root, other, child, leaf]
    );
    assert_eq!(
        truth.object(root).unwrap().children().collect::<Vec<_>>(),
        [child]
    );
    assert_eq!(truth.object(other).unwrap().children().len(), 0);
    let restored = truth.object(child).unwrap();
    assert_eq!(restored.parent(), Some(root));
    assert_eq!(restored.children().collect::<Vec<_>>(), [leaf]);
    assert_eq!(restored.type_key(), node_type);
    assert_eq!(
        truth
            .registry()
            .get_by_key(node_type)
            .unwrap()
            .schema()
            .unwrap()
            .version,
        2
    );
    assert_eq!(
        restored.property("name").unwrap().read::<String>().unwrap(),
        "original"
    );
    assert_eq!(
        restored
            .property("target")
            .unwrap()
            .read::<Option<ObjectId>>()
            .unwrap(),
        Some(root)
    );
    let restored_leaf = truth.object(leaf).unwrap();
    assert_eq!(restored_leaf.parent(), Some(child));
    assert_eq!(
        restored_leaf
            .property("asset")
            .unwrap()
            .read::<Uuid>()
            .unwrap(),
        asset
    );
    assert_eq!(
        restored_leaf
            .property("mode")
            .unwrap()
            .read::<Mode>()
            .unwrap(),
        Mode::Animated { speed: 2 }
    );
    truth.rewind(0).unwrap();
    assert_eq!(truth.objects().len(), 0);
    assert!(truth.journal().is_empty());
}

#[test]
fn rejected_edits_and_noops_preserve_state_and_history() {
    let mut registry = TypeRegistry::default();
    registry.register_reflect::<Node>();
    registry.register_reflect::<Mode>();
    registry.register::<Unregistered>();
    let mut truth = Truth::new(&registry);
    let [root, child, missing] = [1, 2, 3].map(id);
    truth.create::<Node>(root, None).unwrap();
    truth.create::<Node>(child, Some(root)).unwrap();
    truth
        .set_property(child, "name", &"child".to_owned())
        .unwrap();
    let checkpoint = truth.journal().len();

    assert!(
        matches!(truth.create::<Node>(root, None), Err(TruthError::DuplicateObject(id)) if id == root)
    );
    assert!(matches!(
        truth.create::<Unregistered>(missing, None),
        Err(TruthError::UnregisteredType(_))
    ));
    assert!(matches!(
        truth.create::<Mode>(missing, None),
        Err(TruthError::UnsupportedType(_))
    ));
    assert!(
        matches!(truth.reparent(child, Some(missing)), Err(TruthError::MissingObject(id)) if id == missing)
    );
    assert!(
        matches!(truth.reparent(root, Some(child)), Err(TruthError::ParentCycle { object, parent }) if object == root && parent == child)
    );
    assert!(matches!(
        truth.reparent(child, Some(child)),
        Err(TruthError::ParentCycle { .. })
    ));
    assert!(matches!(
        truth.set_property(child, "unknown", &0u32),
        Err(TruthError::UnknownProperty { .. })
    ));
    assert!(matches!(
        truth.set_property(child, "name", &0u32),
        Err(TruthError::PropertyType { .. })
    ));
    assert!(matches!(
        truth.rewind(checkpoint + 1),
        Err(TruthError::InvalidRewind { .. })
    ));
    truth.reparent(child, Some(root)).unwrap();
    truth.clear_property(child, "asset").unwrap();
    truth
        .set_property(child, "name", &"child".to_owned())
        .unwrap();

    assert_eq!(truth.journal().len(), checkpoint);
    assert_eq!(truth.objects().len(), 2);
    let object = truth.object(child).unwrap();
    assert_eq!(object.parent(), Some(root));
    assert_eq!(
        object.property("name").unwrap().read::<String>().unwrap(),
        "child"
    );
}

#[test]
fn values_cannot_be_changed_without_journaling() {
    let mut registry = TypeRegistry::default();
    registry.register_reflect::<Node>();
    let mut truth = Truth::new(&registry);
    let object = id(1);
    truth.create::<Node>(object, None).unwrap();
    let notes = Mutex::new(vec![String::from("authored")]);
    truth.set_property(object, "notes", &notes).unwrap();
    notes.lock().unwrap().clear();
    let saved = truth.object(object).unwrap().property("notes").unwrap();
    saved
        .read::<Mutex<Vec<String>>>()
        .unwrap()
        .lock()
        .unwrap()
        .clear();
    assert_eq!(
        saved
            .read::<Mutex<Vec<String>>>()
            .unwrap()
            .into_inner()
            .unwrap(),
        ["authored"]
    );
    assert!(matches!(
        saved.read::<Vec<String>>(),
        Err(TruthError::PropertyType { .. })
    ));
}
