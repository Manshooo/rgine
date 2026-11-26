use engine_ecs::{ChildOf, Component, Entity, World};
use engine_scene::{CompiledScene, SceneEntities, SceneLoadError, SceneNodeId};
use serde::{Deserialize, Serialize};

const FIXTURE: &str = include_str!("fixtures/compiled.ron");

#[derive(Debug, Serialize, Deserialize)]
struct SpawnData {
    value: u32,
    target: Option<SceneNodeId>,
}

#[derive(Debug, Component)]
struct RuntimeNode {
    value: u32,
    target: Option<Entity>,
}

fn install(
    world: &mut World,
    entity: Entity,
    data: &SpawnData,
    references: &SceneEntities<'_>,
) -> Result<(), &'static str> {
    let target = data
        .target
        .map(|node| references.get(node).ok_or("missing target"))
        .transpose()?;
    world
        .insert(
            entity,
            RuntimeNode {
                value: data.value,
                target,
            },
        )
        .map_err(|_| "entity is stale")?;
    Ok(())
}

fn snapshot(world: &World) -> Vec<(u32, Option<u32>, Option<u32>)> {
    let mut nodes = Vec::new();
    world.query::<&RuntimeNode>().for_each(|entity, node| {
        let parent = world
            .relation_targets::<ChildOf>(entity)
            .next()
            .map(|parent| world.get::<RuntimeNode>(parent).unwrap().value);
        let target = node
            .target
            .map(|target| world.get::<RuntimeNode>(target).unwrap().value);
        nodes.push((node.value, parent, target));
    });
    nodes.sort_unstable();
    nodes
}

#[test]
fn compiled_fixture_loads_hierarchy_and_references_without_truth() {
    let scene: CompiledScene<SpawnData> = ron::from_str(FIXTURE).unwrap();
    let world = scene.instantiate(install).unwrap();
    assert_eq!(world.entity_count(), 3);
    assert_eq!(
        snapshot(&world),
        [
            (4, None, Some(7)),
            (7, Some(4), Some(9)),
            (9, Some(4), None)
        ]
    );
}

#[test]
fn rejects_bad_version_and_hierarchy_before_installing_components() {
    for (fixture, expected) in [
        (
            FIXTURE.replace("schema_version:1", "schema_version:2"),
            "unsupported compiled scene version 2",
        ),
        (
            FIXTURE.replacen("parent:Some(2)", "parent:Some(3)", 1),
            "scene node 0 has missing parent 3",
        ),
        (
            FIXTURE.replace("parent:None", "parent:Some(0)"),
            "relationship would form a cycle",
        ),
    ] {
        let scene: CompiledScene<SpawnData> = ron::from_str(&fixture).unwrap();
        let Err(error) = scene.instantiate::<&str>(|_, _, _, _| {
            panic!("invalid hierarchy must fail before the first callback")
        }) else {
            panic!("invalid scene must fail");
        };
        assert!(error.to_string().contains(expected), "{error}");
    }
    let dangling_target = FIXTURE.replace("target:Some(1)", "target:Some(3)");
    let scene: CompiledScene<SpawnData> = ron::from_str(&dangling_target).unwrap();
    assert!(matches!(
        scene.instantiate(install),
        Err(SceneLoadError::Node {
            source: "missing target",
            ..
        })
    ));
}

#[cfg(feature = "authoring")]
mod authoring {
    use engine_core::{Reflect, TypeRegistry};
    use engine_scene::{ObjectId, SceneCompilation, Truth};

    use super::*;

    const SCOUT: ObjectId = ObjectId::from_u128(1);
    const GOAL: ObjectId = ObjectId::from_u128(2);
    const ROOT: ObjectId = ObjectId::from_u128(3);

    #[allow(dead_code)]
    #[derive(Reflect)]
    struct AuthoredNode {
        value: u32,
        target: ObjectId,
    }

    fn compile(reverse_child_edits: bool) -> SceneCompilation<SpawnData> {
        let mut registry = TypeRegistry::default();
        registry.register_reflect::<AuthoredNode>();
        let mut truth = Truth::new(&registry);
        truth.create::<AuthoredNode>(ROOT, None).unwrap();
        let mut children = [(SCOUT, 7_u32), (GOAL, 9_u32)];
        if reverse_child_edits {
            children.reverse();
        }
        for (id, value) in children {
            truth.create::<AuthoredNode>(id, Some(ROOT)).unwrap();
            truth.set_property(id, "value", &value).unwrap();
        }
        truth.set_property(ROOT, "value", &4_u32).unwrap();
        truth.set_property(ROOT, "target", &SCOUT).unwrap();
        truth.set_property(SCOUT, "target", &GOAL).unwrap();
        truth
            .compile(|_, node, references| {
                let value = node.property("value").unwrap().read::<u32>().unwrap();
                let target = node
                    .property("target")
                    .map(|target| {
                        references
                            .get(target.read::<ObjectId>().unwrap())
                            .ok_or("missing target")
                    })
                    .transpose()?;
                Ok::<_, &'static str>(SpawnData { value, target })
            })
            .unwrap()
    }

    #[test]
    fn independent_full_compiles_produce_identical_artifacts_and_worlds() {
        let first = compile(false).into_compiled();
        let second = compile(true).into_compiled();
        assert_eq!(ron::to_string(&first).unwrap(), FIXTURE.trim_end());
        assert_eq!(
            ron::to_string(&first).unwrap(),
            ron::to_string(&second).unwrap()
        );
        assert_eq!(
            snapshot(&first.instantiate(install).unwrap()),
            snapshot(&second.instantiate(install).unwrap())
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn development_trace_resolves_both_directions() {
        let (world, trace) = compile(false).instantiate_traced(install).unwrap();
        let scout = trace.entity(SCOUT).unwrap();
        let goal = trace.entity(GOAL).unwrap();
        let root = trace.entity(ROOT).unwrap();
        assert_eq!(trace.object(scout), Some(SCOUT));
        assert_eq!(trace.object(goal), Some(GOAL));
        assert_eq!(trace.object(root), Some(ROOT));
        assert!(world.relation_sources::<ChildOf>(root).eq([scout, goal]));
        assert_eq!(world.get::<RuntimeNode>(scout).unwrap().target, Some(goal));
    }
}
