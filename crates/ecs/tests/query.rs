use std::panic::{AssertUnwindSafe, catch_unwind};

use engine_ecs::{Component, Entity, With, Without, World};

#[derive(Component, Debug, PartialEq)]
struct Position(i32);

#[derive(Component, Debug)]
struct Velocity(i32);

#[derive(Component, Debug)]
#[component(storage = "sparse")]
struct Selected(i32);

#[derive(Component, Debug)]
struct Disabled;

fn with_position(world: &mut World, position: i32) -> Entity {
    let entity = world.spawn_empty();
    world.insert(entity, Position(position)).unwrap();
    entity
}

#[test]
fn visits_archetypes_in_creation_order_across_worlds() {
    fn order() -> [(Entity, i32); 3] {
        let mut world = World::new();
        let moved = with_position(&mut world, 10);
        world.insert(moved, Velocity(5)).unwrap();
        let first = with_position(&mut world, 20);
        let second = with_position(&mut world, 30);

        let mut visited = [(moved, 0); 3];
        let mut rows = visited.iter_mut();
        world.query::<&Position>().for_each(|entity, position| {
            *rows.next().unwrap() = (entity, position.0);
        });
        assert!(rows.next().is_none());
        assert_eq!(visited, [(first, 20), (second, 30), (moved, 10)]);
        visited
    }

    assert_eq!(order(), order());
}

#[test]
fn filters_and_sparse_joins_keep_dense_rows_aligned() {
    let mut world = World::new();
    let missing_sparse = with_position(&mut world, 10);
    let selected = with_position(&mut world, 20);
    let disabled = with_position(&mut world, 30);
    world.insert(selected, Selected(2)).unwrap();
    world.insert(disabled, Selected(3)).unwrap();
    world.insert(disabled, Disabled).unwrap();

    let mut query = world
        .query_filtered::<(&mut Position, &mut Selected), (With<Selected>, Without<Disabled>)>();
    let mut visited = None;
    query.for_each(|entity, (position, selection)| {
        assert_eq!(visited.replace(entity), None);
        position.0 += selection.0;
        selection.0 += 1;
    });
    assert_eq!(visited, Some(selected));
    drop(query);
    assert_eq!(
        world.get::<Position>(missing_sparse).as_deref(),
        Some(&Position(10))
    );
    assert_eq!(
        world.get::<Position>(selected).as_deref(),
        Some(&Position(22))
    );
    assert_eq!(
        world.get::<Position>(disabled).as_deref(),
        Some(&Position(30))
    );

    let mut sparse = world.query_filtered::<&mut Selected, With<Disabled>>();
    assert!(sparse.get(selected).is_none());
    sparse.get_mut(disabled).unwrap().0 = 8;
    assert_eq!(sparse.get(disabled).unwrap().0, 8);
    drop(sparse);
    assert_eq!(world.get::<Selected>(selected).unwrap().0, 3);
}

#[test]
fn conflicting_borrows_panic_identically_across_disjoint_archetypes() {
    let mut world = World::new();
    with_position(&mut world, 10);
    let moving = with_position(&mut world, 20);
    world.insert(moving, Velocity(5)).unwrap();

    let first = world.query_filtered::<&mut Position, With<Velocity>>();
    let same = catch_unwind(AssertUnwindSafe(|| {
        world.query_filtered::<&mut Position, With<Velocity>>();
    }))
    .unwrap_err();
    let disjoint = catch_unwind(AssertUnwindSafe(|| {
        world.query_filtered::<&mut Position, Without<Velocity>>();
    }))
    .unwrap_err();
    assert_eq!(
        same.downcast_ref::<String>(),
        disjoint.downcast_ref::<String>()
    );
    assert!(
        same.downcast_ref::<String>()
            .unwrap()
            .contains("conflicting borrow of component")
    );

    let velocities = world.query::<&Velocity>();
    assert_eq!(velocities.get(moving).unwrap().0, 5);
    drop(velocities);
    drop(first);
    let shared = world.get::<Position>(moving).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| world.query::<&mut Position>())).is_err());
    drop(shared);
    assert!(
        catch_unwind(AssertUnwindSafe(
            || world.query::<(&Disabled, &mut Disabled)>()
        ))
        .is_err()
    );
    let mut pair = world.query::<(&Position, &Position)>();
    pair.for_each(|_, (a, b)| assert_eq!(a, b));
}

#[test]
fn disjoint_mutable_access_preserves_input_order_and_checks_entities() {
    let mut world = World::new();
    let first = with_position(&mut world, 10);
    let second = with_position(&mut world, 20);
    let third = with_position(&mut world, 30);
    world.insert(second, Velocity(1)).unwrap();
    let empty = world.spawn_empty();
    let stale = world.spawn_empty();
    assert!(world.despawn(stale));
    let reused = with_position(&mut world, 40);
    assert_eq!(stale.index(), reused.index());

    let mut query = world.query::<&mut Position>();
    let [c, b, a] = query.get_disjoint_mut([third, second, first]).unwrap();
    assert_eq!((c.0, b.0, a.0), (30, 20, 10));
    c.0 += 3;
    b.0 += 2;
    a.0 += 1;
    assert_eq!(query.get(first), Some(&Position(11)));
    assert_eq!(query.get(second), Some(&Position(22)));
    assert_eq!(query.get(third), Some(&Position(33)));
    assert!(query.get_disjoint_mut([first, first]).is_none());
    assert!(query.get_disjoint_mut([first, empty]).is_none());
    assert!(query.get_disjoint_mut([first, stale]).is_none());
    assert!(query.get_disjoint_mut([]).is_some());
    drop(query);

    let mut filtered = world.query_filtered::<&mut Position, With<Velocity>>();
    assert!(filtered.get_disjoint_mut([second, first]).is_none());
    assert_eq!(filtered.get_disjoint_mut([second]).unwrap()[0].0, 22);
    assert!(
        world
            .query::<&mut Disabled>()
            .get_disjoint_mut([])
            .is_some()
    );
}

#[test]
fn sparse_disjoint_access_updates_requested_rows_after_swap_remove() {
    let mut world = World::new();
    let entities = [
        world.spawn_empty(),
        world.spawn_empty(),
        world.spawn_empty(),
    ];
    for (index, entity) in entities.into_iter().enumerate() {
        world.insert(entity, Selected(index as i32)).unwrap();
    }
    world.remove::<Selected>(entities[0]).unwrap();
    let mut query = world.query::<&mut Selected>();
    let [second, third] = query.get_disjoint_mut([entities[1], entities[2]]).unwrap();
    assert_eq!((second.0, third.0), (1, 2));
    second.0 = 4;
    third.0 = 5;
    assert_eq!(query.get(entities[1]).unwrap().0, 4);
    assert_eq!(query.get(entities[2]).unwrap().0, 5);
    assert!(query.get_disjoint_mut([entities[2], entities[2]]).is_none());
    assert!(query.get_disjoint_mut([entities[0], entities[2]]).is_none());
}

#[test]
fn panicking_visitors_release_the_component_borrow() {
    let mut world = World::new();
    let entity = with_position(&mut world, 10);
    let panic = catch_unwind(AssertUnwindSafe(|| {
        world.query::<&mut Position>().for_each(|_, position| {
            position.0 += 1;
            panic!("abort the system");
        });
    }));
    assert!(panic.is_err());
    assert_eq!(
        world.get::<Position>(entity).as_deref(),
        Some(&Position(11))
    );
    world.query::<&mut Position>().get_mut(entity).unwrap().0 = 12;
    assert_eq!(world.remove::<Position>(entity), Some(Position(12)));
}
