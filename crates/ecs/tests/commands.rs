use engine_ecs::{Added, Commands, Component, Entity, World};

#[derive(Component, Debug)]
struct Position(i32);

#[derive(Component, Debug)]
#[component(storage = "sparse")]
struct Selected;

fn positions(world: &World) -> Vec<(Entity, i32)> {
    let mut rows = Vec::new();
    world
        .query::<&Position>()
        .for_each(|entity, position| rows.push((entity, position.0)));
    rows
}

#[test]
fn structural_changes_become_visible_at_the_boundary() {
    let mut world = World::new();
    let old = world.spawn_empty();
    world.insert(old, Position(1)).unwrap();
    world.insert(old, Selected).unwrap();
    let last_run = world.tick();
    world.advance_tick();

    let mut commands = Commands::default();
    {
        let mut query = world.query::<&Position>();
        query.for_each(|entity, position| {
            commands.remove::<Position>(entity);
            commands.remove::<Selected>(entity);
            commands.spawn_empty().insert(Position(position.0 + 1));
        });
        assert_eq!(query.get(old).unwrap().0, 1);
        assert!(world.get::<Selected>(old).is_some());
        assert_eq!(world.entity_count(), 1);
    }

    commands.apply(&mut world);
    assert!(world.get::<Position>(old).is_none());
    assert!(world.get::<Selected>(old).is_none());
    assert_eq!(world.entity_count(), 2);
    let [(new, 2)] = positions(&world)[..] else {
        panic!("expected only the spawned position");
    };
    assert!(
        world
            .query_filtered_since::<&Position, Added<Position>>(last_run)
            .get(new)
            .is_some()
    );

    commands.apply(&mut world);
    assert_eq!(world.entity_count(), 2);
    assert_eq!(positions(&world), vec![(new, 2)]);
}

#[test]
fn commands_apply_in_recording_order() {
    fn run() -> Vec<(Entity, i32)> {
        let mut world = World::new();
        let entity = world.spawn_empty();
        let mut commands = Commands::default();
        commands.insert(entity, Position(10));
        commands.remove::<Position>(entity);
        commands.insert(entity, Position(20));
        commands.spawn_empty().insert(Position(30)).insert(Selected);
        commands.spawn_empty().insert(Position(40));
        commands.apply(&mut world);

        let rows = positions(&world);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], (entity, 20));
        assert_eq!(rows[1].1, 30);
        assert_eq!(rows[2].1, 40);
        assert!(world.get::<Selected>(rows[1].0).is_some());
        assert!(world.get::<Selected>(rows[2].0).is_none());
        rows
    }

    assert_eq!(run(), run());
}

#[test]
fn commands_for_a_despawned_entity_cannot_touch_its_reused_slot() {
    let mut world = World::new();
    let old = world.spawn_empty();
    world.insert(old, Position(1)).unwrap();

    let mut commands = Commands::default();
    commands.despawn(old);
    commands.spawn_empty().insert(Position(2));
    commands.remove::<Position>(old);
    commands.insert(old, Position(3));
    commands.despawn(old);
    commands.apply(&mut world);

    assert!(!world.contains(old));
    assert_eq!(world.entity_count(), 1);
    let [(new, 2)] = positions(&world)[..] else {
        panic!("expected the replacement entity's position");
    };
    assert_eq!(new.index(), old.index());
    assert_ne!(new.generation(), old.generation());
}
