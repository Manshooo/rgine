use engine_ecs::{Added, Changed, Component, Entity, QueryFilter, Tick, With, World};

#[derive(Debug, Component)]
struct Position(u32);

#[derive(Debug, Component)]
#[component(storage = "sparse")]
struct Selected(u32);

#[derive(Debug, Component)]
struct Marker;

fn matches<T: Component, F: QueryFilter>(world: &World, since: Tick) -> Vec<Entity> {
    let mut entities = Vec::new();
    world
        .query_filtered_since::<&T, F>(since)
        .for_each(|entity, _| entities.push(entity));
    entities
}

#[test]
fn system_running_every_third_tick_sees_changes_from_both_skipped_ticks() {
    let mut world = World::new();
    let first = world.spawn_empty();
    let second = world.spawn_empty();
    let unchanged = world.spawn_empty();
    world.insert(first, Position(1)).unwrap();
    world.insert(second, Position(2)).unwrap();
    world.insert(unchanged, Position(3)).unwrap();
    world.insert(unchanged, Selected(4)).unwrap();
    assert_eq!(
        matches::<Position, Added<Position>>(&world, Tick::ZERO),
        [first, second, unchanged],
    );
    let last_run = world.tick();

    world.advance_tick();
    world.get_mut::<Position>(first).unwrap().0 += 1;

    world.advance_tick();
    world.insert(second, Position(5)).unwrap();
    world.insert(second, Selected(6)).unwrap();

    world.advance_tick();
    assert_eq!(
        matches::<Position, Changed<Position>>(&world, last_run),
        [first, second]
    );
    assert!(matches::<Position, Added<Position>>(&world, last_run).is_empty());
    assert_eq!(
        matches::<Position, Added<Selected>>(&world, last_run),
        [second]
    );
    assert!(matches::<Position, Changed<Position>>(&world, world.tick()).is_empty());
    assert_eq!(
        world
            .query_filtered::<&Position, Changed<Position>>()
            .get(first)
            .map(|value| value.0),
        None
    );
}

#[test]
fn change_ticks_follow_table_migrations_and_sparse_swap_removes() {
    let mut world = World::new();
    let removed = world.spawn_empty();
    let moved = world.spawn_empty();
    for entity in [removed, moved] {
        world.insert(entity, Position(1)).unwrap();
        world.insert(entity, Selected(2)).unwrap();
    }
    let added_at = world.tick();

    world.advance_tick();
    world.insert(moved, Selected(3)).unwrap();
    world.get_mut::<Position>(moved).unwrap().0 = 4;

    let changed_at = world.tick();
    world.advance_tick();
    world.despawn(removed);
    world.insert(moved, Marker).unwrap();
    assert!(matches::<Position, Changed<Position>>(&world, changed_at).is_empty());
    assert!(matches::<Selected, Changed<Selected>>(&world, changed_at).is_empty());
    assert_eq!(
        matches::<Position, Changed<Position>>(&world, added_at),
        [moved]
    );
    assert_eq!(
        matches::<Selected, Changed<Selected>>(&world, added_at),
        [moved]
    );
    assert!(matches::<Selected, Added<Selected>>(&world, added_at).is_empty());

    world.remove::<Selected>(moved).unwrap();
    world.insert(moved, Selected(5)).unwrap();
    assert_eq!(
        matches::<Selected, Added<Selected>>(&world, changed_at),
        [moved]
    );
    assert_eq!(
        matches::<Selected, Changed<Selected>>(&world, changed_at),
        [moved]
    );
    assert_eq!(world.get::<Selected>(moved).unwrap().0, 5);
}

#[test]
fn mutable_queries_mark_only_values_actually_exposed() {
    let mut world = World::new();
    let missing_sparse = world.spawn_empty();
    let filtered = world.spawn_empty();
    let matched = world.spawn_empty();
    for entity in [missing_sparse, filtered, matched] {
        world.insert(entity, Position(1)).unwrap();
    }
    world.insert(filtered, Selected(2)).unwrap();
    world.insert(matched, Selected(2)).unwrap();
    world.insert(missing_sparse, Marker).unwrap();
    world.insert(matched, Marker).unwrap();

    let last_run = world.tick();
    world.advance_tick();
    let mut query = world.query_filtered::<(&mut Position, &mut Selected), With<Marker>>();
    query.for_each(|entity, _| assert_eq!(entity, matched));
    drop(query);
    assert_eq!(
        matches::<Position, Changed<Position>>(&world, last_run),
        [matched]
    );
    assert_eq!(
        matches::<Position, Changed<Selected>>(&world, last_run),
        [matched]
    );

    let last_run = world.tick();
    world.advance_tick();
    let mut query = world.query::<&mut Position>();
    assert!(query.get(filtered).is_some());
    assert!(query.get_disjoint_mut([filtered, filtered]).is_none());
    drop(query);
    assert!(matches::<Position, Changed<Position>>(&world, last_run).is_empty());

    let mut query = world.query::<&mut Position>();
    assert!(query.get_mut(filtered).is_some());
    assert!(query.get_disjoint_mut([missing_sparse, matched]).is_some());
    drop(query);
    assert_eq!(
        matches::<Position, Changed<Position>>(&world, last_run).len(),
        3
    );
}

#[test]
fn a_mutable_change_filter_reads_ticks_before_marking_the_row() {
    let mut world = World::new();
    let old = world.spawn_empty();
    world.insert(old, Selected(1)).unwrap();
    let last_run = world.tick();
    world.advance_tick();
    let new = world.spawn_empty();
    world.insert(new, Selected(2)).unwrap();

    let mut query = world.query_filtered_since::<&mut Selected, Changed<Selected>>(last_run);
    assert!(query.get_mut(old).is_none());
    assert!(query.get_disjoint_mut([old, new]).is_none());
    query.for_each(|entity, value| {
        assert_eq!(entity, new);
        value.0 += 1;
    });
    drop(query);
    assert_eq!(
        matches::<Selected, Changed<Selected>>(&world, last_run),
        [new]
    );
}
