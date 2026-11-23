use engine_ecs::{Changed, Component, Schedule, World};

#[derive(Component, Debug, PartialEq, Eq)]
struct Value(u32);

#[test]
fn schedule_seals_structural_changes_until_the_boundary() {
    fn run(input: u32) -> (Vec<&'static str>, u32) {
        let mut world = World::new();
        let entity = world.spawn_empty();
        let mut order = Vec::new();
        let mut schedule = Schedule::<Vec<&'static str>>::default();
        let first = schedule.add_stage();
        let second = schedule.add_stage();

        schedule.add_system(second, move |world, _, order, _| {
            order.push("second stage");
            assert_eq!(world.get::<Value>(entity).unwrap().0, input);
            world.query::<&mut Value>().get_mut(entity).unwrap().0 += 1;
        });
        schedule.add_system(first, move |_, commands, order, _| {
            order.push("first system");
            commands.insert(entity, Value(input));
        });
        schedule.add_system(first, move |world, _, order, _| {
            order.push("second system");
            assert!(world.get::<Value>(entity).is_none());
        });

        schedule.run(&mut world, &mut order);
        (order, world.get::<Value>(entity).unwrap().0)
    }

    let expected = (vec!["first system", "second system", "second stage"], 42);
    assert_eq!(run(41), expected);
    assert_eq!(run(41), expected);
}

#[test]
fn an_early_reader_observes_later_system_and_boundary_writes_on_its_next_run() {
    let mut world = World::new();
    let entity = world.spawn_empty();
    let queued = world.spawn_empty();
    world.insert(entity, Value(1)).unwrap();
    let mut schedule = Schedule::<Vec<u32>>::default();
    let stage = schedule.add_stage();
    schedule.add_system(stage, |world, _, observed, last_run| {
        world
            .query_filtered_since::<&Value, Changed<Value>>(last_run)
            .for_each(|_, value| observed.push(value.0));
    });
    schedule.add_system(stage, move |world, commands, _, _| {
        world.query::<&mut Value>().get_mut(entity).unwrap().0 += 1;
        commands.insert(queued, Value(10));
    });

    let mut observed = Vec::new();
    schedule.run(&mut world, &mut observed);
    assert_eq!(observed, [1]);
    observed.clear();
    schedule.run(&mut world, &mut observed);
    assert_eq!(observed, [2, 10]);
}
