#![cfg(feature = "authoring")]

use engine_core::{Reflect, TypeRegistry};
use engine_ecs::{Changed, ChildOf, Component, Entity, World};
use engine_scene::{
    CompileError, IncrementalScene, ObjectId, SceneEntities, SceneEntityCommands, SceneNodeId,
    SceneReferences, SceneTypeName, SceneUpdateError, Truth, TruthObject,
};

const LEFT: ObjectId = ObjectId::from_u128(1);
const RIGHT: ObjectId = ObjectId::from_u128(2);
const BRANCH: ObjectId = ObjectId::from_u128(3);
const LEAF: ObjectId = ObjectId::from_u128(4);
const SIBLING: ObjectId = ObjectId::from_u128(5);
const OBSERVER: ObjectId = ObjectId::from_u128(6);

#[allow(dead_code)]
#[derive(Reflect)]
struct Node {
    value: u32,
    target: ObjectId,
    highlighted: bool,
}

struct SpawnData {
    value: u32,
    target: Option<SceneNodeId>,
    children: usize,
    highlighted: bool,
}

#[derive(Debug, Component)]
struct RuntimeNode {
    value: u32,
    target: Option<Entity>,
    children: usize,
}

#[derive(Debug, Component)]
struct Highlight;

#[derive(Debug, Component)]
struct SimulationCounter(u32);

type TestResult<T> = Result<T, &'static str>;

fn registry() -> TypeRegistry {
    let mut registry = TypeRegistry::default();
    registry
        .register_reflect::<Node>()
        .insert_metadata(SceneTypeName("test.node"));
    registry
}

fn add(truth: &mut Truth<'_>, id: ObjectId, parent: Option<ObjectId>, value: u32) {
    truth.create::<Node>(id, parent).unwrap();
    truth.set_property(id, "value", &value).unwrap();
}

fn lower(
    _: ObjectId,
    node: &TruthObject,
    references: &SceneReferences<'_>,
) -> TestResult<SpawnData> {
    Ok(SpawnData {
        value: node
            .property("value")
            .ok_or("missing value")?
            .read()
            .map_err(|_| "invalid value")?,
        target: node
            .property("target")
            .and_then(|value| references.get(value.read().unwrap())),
        children: node.children().len(),
        highlighted: node
            .property("highlighted")
            .is_some_and(|value| value.read().unwrap()),
    })
}

fn runtime(data: &SpawnData, references: &SceneEntities<'_>) -> TestResult<RuntimeNode> {
    let target = data
        .target
        .map(|index| references.get(index).ok_or("missing scene entity"))
        .transpose()?;
    Ok(RuntimeNode {
        value: data.value,
        target,
        children: data.children,
    })
}

fn prepare(
    edits: &mut SceneEntityCommands<'_>,
    data: &SpawnData,
    references: &SceneEntities<'_>,
) -> TestResult<()> {
    edits.insert(runtime(data, references)?);
    if data.highlighted {
        edits.insert(Highlight);
    } else {
        edits.remove::<Highlight>();
    }
    Ok(())
}

fn install(
    world: &mut World,
    entity: Entity,
    data: &SpawnData,
    references: &SceneEntities<'_>,
) -> TestResult<()> {
    world
        .insert(entity, runtime(data, references)?)
        .map_err(|_| "stale runtime node")?;
    if data.highlighted {
        world
            .insert(entity, Highlight)
            .map_err(|_| "stale highlight")?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Observed {
    value: u32,
    target: Option<u32>,
    parent: Option<u32>,
    children: usize,
    highlighted: bool,
}

fn snapshot(world: &World) -> Vec<Observed> {
    let mut output = Vec::new();
    world.query::<&RuntimeNode>().for_each(|entity, node| {
        let value = |entity| world.get::<RuntimeNode>(entity).unwrap().value;
        output.push(Observed {
            value: node.value,
            target: node.target.map(value),
            parent: world.relation_targets::<ChildOf>(entity).next().map(value),
            children: node.children,
            highlighted: world.get::<Highlight>(entity).is_some(),
        });
    });
    output.sort_unstable();
    output
}

fn update<'registry>(
    scene: &mut IncrementalScene<'registry>,
    truth: &Truth<'registry>,
) -> Vec<ObjectId> {
    let mut affected = Vec::new();
    scene
        .update(
            truth,
            |id, node, references| {
                affected.push(id);
                lower(id, node, references)
            },
            prepare,
        )
        .unwrap();
    let full = truth
        .compile(lower)
        .unwrap()
        .into_compiled()
        .instantiate(install)
        .unwrap();
    assert_eq!(snapshot(scene.world()), snapshot(&full));
    assert_eq!(scene.world().entity_count(), full.entity_count());
    affected
}

#[test]
fn keeps_surviving_entities_and_only_recompiles_affected_nodes() {
    let registry = registry();
    let mut truth = Truth::new(&registry);
    add(&mut truth, LEFT, None, 10);
    add(&mut truth, RIGHT, None, 20);
    add(&mut truth, BRANCH, Some(LEFT), 11);
    add(&mut truth, LEAF, Some(BRANCH), 12);
    add(&mut truth, SIBLING, Some(RIGHT), 21);
    add(&mut truth, OBSERVER, None, 30);
    truth.set_property(OBSERVER, "target", &LEAF).unwrap();
    truth.set_property(BRANCH, "highlighted", &true).unwrap();

    let mut scene = IncrementalScene::new(&registry);
    assert_eq!(
        update(&mut scene, &truth),
        [LEFT, RIGHT, BRANCH, LEAF, SIBLING, OBSERVER]
    );
    let branch = scene.entity(BRANCH).unwrap();
    let leaf = scene.entity(LEAF).unwrap();
    let sibling = scene.entity(SIBLING).unwrap();
    let observer = scene.entity(OBSERVER).unwrap();
    scene
        .world_mut()
        .insert(leaf, SimulationCounter(42))
        .unwrap();
    let last_read = scene.world().tick();

    truth.set_property(BRANCH, "value", &13_u32).unwrap();
    truth.clear_property(BRANCH, "highlighted").unwrap();
    assert_eq!(update(&mut scene, &truth), [BRANCH, LEAF]);
    assert_eq!(scene.entity(BRANCH), Some(branch));
    assert_eq!(scene.entity(LEAF), Some(leaf));
    let mut changed = Vec::new();
    scene
        .world()
        .query_filtered_since::<&RuntimeNode, Changed<RuntimeNode>>(last_read)
        .for_each(|entity, _| changed.push(entity));
    changed.sort_unstable();
    assert_eq!(changed, [branch, leaf]);
    assert_eq!(scene.world().get::<SimulationCounter>(leaf).unwrap().0, 42);

    truth.reparent(BRANCH, Some(RIGHT)).unwrap();
    let saved = truth.to_ron().unwrap();
    let mut truth = Truth::from_ron(&registry, &saved).unwrap();
    assert!(truth.journal().is_empty());
    assert_eq!(update(&mut scene, &truth), [LEFT, RIGHT, BRANCH, LEAF]);
    assert_eq!(scene.entity(BRANCH), Some(branch));
    assert_eq!(scene.entity(SIBLING), Some(sibling));
    assert!(update(&mut scene, &truth).is_empty());

    truth.delete(BRANCH).unwrap();
    assert_eq!(update(&mut scene, &truth), [RIGHT, OBSERVER]);
    assert_eq!(scene.object(leaf), None);
    assert_eq!(scene.entity(BRANCH), None);
    assert_eq!(scene.entity(OBSERVER), Some(observer));
    assert!(!scene.world().contains(leaf));
    assert_eq!(
        scene.world().get::<RuntimeNode>(observer).unwrap().target,
        None
    );

    add(&mut truth, LEAF, Some(LEFT), 14);
    assert_eq!(update(&mut scene, &truth), [LEFT, LEAF, OBSERVER]);
    let new_leaf = scene.entity(LEAF).unwrap();
    assert_ne!(new_leaf, leaf);
    assert_eq!(scene.object(new_leaf), Some(LEAF));
    assert_eq!(scene.entity(SIBLING), Some(sibling));
    assert_eq!(
        scene.world().get::<RuntimeNode>(observer).unwrap().target,
        Some(new_leaf)
    );
}

#[test]
fn failure_keeps_previous_world_and_a_reparenting_retry_applies_all_edges() {
    let registry = registry();
    let mut truth = Truth::new(&registry);
    add(&mut truth, LEFT, None, 1);
    add(&mut truth, RIGHT, Some(LEFT), 2);
    let mut scene = IncrementalScene::new(&registry);
    update(&mut scene, &truth);
    let before = snapshot(scene.world());
    let before_tick = scene.world().tick();
    let left = scene.entity(LEFT).unwrap();
    let right = scene.entity(RIGHT).unwrap();

    truth.reparent(RIGHT, None).unwrap();
    truth.reparent(LEFT, Some(RIGHT)).unwrap();
    truth.set_property(LEFT, "value", &10_u32).unwrap();
    add(&mut truth, BRANCH, Some(RIGHT), 3);
    let rejected = scene.update(
        &truth,
        |id, node, references| {
            if id == BRANCH {
                return Err("bad authored data");
            }
            lower(id, node, references)
        },
        prepare,
    );
    assert!(matches!(
        rejected,
        Err(SceneUpdateError::Compile(CompileError::Object {
            object: BRANCH,
            ..
        }))
    ));
    assert_eq!(snapshot(scene.world()), before);

    let rejected = scene.update(&truth, lower, |edits, data, references| {
        prepare(edits, data, references)?;
        if data.value == 3 {
            return Err("missing resource");
        }
        Ok(())
    });
    assert!(matches!(
        rejected,
        Err(SceneUpdateError::Prepare { object: BRANCH, .. })
    ));
    assert_eq!(snapshot(scene.world()), before);
    assert_eq!(scene.world().entity_count(), 2);
    assert_eq!(scene.world().tick(), before_tick);
    assert_eq!(scene.entity(BRANCH), None);

    assert_eq!(update(&mut scene, &truth), [LEFT, RIGHT, BRANCH]);
    assert_eq!(scene.entity(LEFT), Some(left));
    assert_eq!(scene.entity(RIGHT), Some(right));
    assert!(scene.world().relation_targets::<ChildOf>(left).eq([right]));
    assert_eq!(scene.object(right), Some(RIGHT));
}
