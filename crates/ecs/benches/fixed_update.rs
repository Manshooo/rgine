use std::time::{Duration, Instant};

use engine_ecs::{Component, Schedule, World};

#[derive(Component, Debug)]
struct Position(u32);

#[derive(Component, Debug)]
struct Velocity(u32);

fn measure(entity_count: u32) -> Duration {
    let mut world = World::new();
    for _ in 0..entity_count {
        let entity = world.spawn_empty();
        world.insert(entity, Position(0)).unwrap();
        world.insert(entity, Velocity(1)).unwrap();
    }
    let mut schedule = Schedule::default();
    let update = schedule.add_stage();
    schedule.add_system(update, |world, _, (), _| {
        world
            .query::<(&mut Position, &Velocity)>()
            .for_each(|_, (position, velocity)| {
                position.0 += velocity.0;
            });
    });

    let warmup = 10;
    for _ in 0..warmup {
        schedule.run(&mut world, &mut ());
    }
    let steps_per_sample = (2_000_000 / entity_count).max(20);
    let mut samples = [Duration::ZERO; 5];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..steps_per_sample {
            schedule.run(&mut world, &mut ());
        }
        *sample = start.elapsed() / steps_per_sample;
    }
    let expected = warmup + steps_per_sample * samples.len() as u32;
    let mut visited = 0;
    world.query::<&Position>().for_each(|_, position| {
        assert_eq!(position.0, expected);
        visited += 1;
    });
    assert_eq!(visited, entity_count);
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn main() {
    for count in [1_000, 100_000] {
        println!(
            "{count} entities: {:?} median per fixed update",
            measure(count)
        );
    }
}
