use engine_ecs::{ChildOf, Commands, Component, Relation, RelationError, World};

enum DependsOn {}
impl Relation for DependsOn {}

#[test]
fn repeated_edges_cycles_reparenting_and_despawn_keep_both_indices_consistent() {
    let mut world = World::new();
    let first = world.spawn_empty();
    let second = world.spawn_empty();
    let third = world.spawn_empty();

    assert_eq!(world.add_relation::<DependsOn>(first, third), Ok(true));
    assert_eq!(world.add_relation::<DependsOn>(first, second), Ok(true));
    assert_eq!(world.add_relation::<DependsOn>(first, third), Ok(false));
    assert!(
        world
            .relation_targets::<DependsOn>(first)
            .eq([second, third])
    );
    assert_eq!(
        world.add_relation::<DependsOn>(third, first),
        Err(RelationError::Cycle)
    );
    world.add_relation::<DependsOn>(second, third).unwrap();
    world.add_relation::<ChildOf>(first, third).unwrap();
    world.add_relation::<ChildOf>(first, second).unwrap();
    assert_eq!(world.relation_sources::<ChildOf>(third).next(), None);
    assert_eq!(
        world.add_relation::<ChildOf>(second, first),
        Err(RelationError::Cycle)
    );
    assert!(world.relation_targets::<ChildOf>(first).eq([second]));

    assert!(world.despawn(second));
    let reused = world.spawn_empty();
    assert_eq!(reused.index(), second.index());
    assert!(world.relation_targets::<DependsOn>(first).eq([third]));
    assert!(world.relation_sources::<DependsOn>(third).eq([first]));
    assert_eq!(world.relation_targets::<ChildOf>(first).next(), None);
    assert_eq!(world.relation_sources::<ChildOf>(reused).next(), None);
    assert_eq!(
        world.add_relation::<ChildOf>(first, second),
        Err(RelationError::StaleEntity(second))
    );
}

#[derive(Component, Debug)]
struct Transform {
    local: i32,
    global: i32,
}

#[test]
fn hierarchy_propagates_through_the_accessor_in_one_archetype() {
    let mut world = World::new();
    let root = world.spawn_empty();
    let child = world.spawn_empty();
    for (entity, local) in [(root, 2), (child, 5)] {
        world
            .insert(
                entity,
                Transform {
                    local,
                    global: local,
                },
            )
            .unwrap();
    }
    let mut commands = Commands::default();
    commands.add_relation::<ChildOf>(child, root);
    commands
        .spawn_empty()
        .insert(Transform {
            local: 7,
            global: 0,
        })
        .add_relation::<ChildOf>(child);
    assert_eq!(world.relation_sources::<ChildOf>(root).next(), None);
    commands.apply(&mut world);
    let leaf = world.relation_sources::<ChildOf>(child).next().unwrap();

    let mut transforms = world.query::<&mut Transform>();
    for parent in [root, child] {
        for child in world.relation_sources::<ChildOf>(parent) {
            let [parent, child] = transforms.get_disjoint_mut([parent, child]).unwrap();
            child.global = parent.global + child.local;
        }
    }
    assert_eq!(transforms.get(leaf).unwrap().global, 14);
}
