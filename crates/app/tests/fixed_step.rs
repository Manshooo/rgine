use std::num::NonZeroU32;
use std::time::Duration;

use engine_app::ecs::Component;
use engine_app::{App, FixedStep, Plugin};

#[derive(Component, Debug)]
struct Value(u64);

struct History(Vec<u64>);
struct TestPlugin;

impl Plugin for TestPlugin {
    fn build(&self, app: &mut App) {
        let world = app.world_mut();
        world.register_component::<Value>();
        let entity = world.spawn_empty();
        world.insert(entity, Value(0)).unwrap();
        app.insert_resource(History(Vec::new()));
        app.add_system(move |world, _, step, _| {
            assert_eq!(step.delta(), Duration::from_millis(10));
            let index = step.index();
            step.resources_mut()
                .get_mut::<History>()
                .unwrap()
                .0
                .push(index);
            world.query::<&mut Value>().get_mut(entity).unwrap().0 += index;
        });
    }
}

#[test]
fn fixed_steps_and_plugin_state_reproduce_without_a_window() {
    fn run(frames: &[u64]) -> (Vec<u32>, Vec<u64>, u64) {
        let mut simulation = App::new()
            .with_fixed_step(FixedStep::new(
                Duration::from_millis(10),
                NonZeroU32::new(2).unwrap(),
            ))
            .add_plugin(TestPlugin)
            .into_simulation();
        let step_counts = frames
            .iter()
            .map(|ms| simulation.advance(Duration::from_millis(*ms)))
            .collect();
        let mut value = 0;
        simulation
            .world()
            .query::<&Value>()
            .for_each(|_, stored| value = stored.0);
        (
            step_counts,
            simulation.resources().get::<History>().unwrap().0.clone(),
            value,
        )
    }

    let expected = (vec![0, 2, 2, 1], vec![1, 2, 3, 4, 5], 15);
    assert_eq!(run(&[6, 14, 45, 5]), expected);
    assert_eq!(run(&[6, 14, 45, 5]), expected);
    assert_eq!(run(&[10, 10, 10, 10, 10]).1, expected.1);
}
