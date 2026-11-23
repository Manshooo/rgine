use std::num::NonZeroU32;
use std::time::Duration;

use engine_app::ecs::{Component, Entity, World};
use engine_app::platform::{
    Commands, Input, InputEvent, InputTimestamp, KeyCode, PhysicalKey, TouchPhase, WindowDesc,
};
use engine_app::{App, FixedStep, InputRecording, Plugin, ReplayError, Simulation};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
struct Position(i32);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
struct Velocity(i32);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
struct Destination(i32, i32);

struct Controls(i32);
struct Game;

impl Plugin for Game {
    fn build(&self, app: &mut App) {
        let world = app.world_mut();
        let mut actors = Vec::new();
        for start in [0, 10] {
            let actor = world.spawn_empty();
            world.insert(actor, Position(start)).unwrap();
            world.insert(actor, Velocity(0)).unwrap();
            actors.push(actor);
        }
        app.insert_resource(Controls(0));
        app.add_system(move |world, commands, step, _| {
            let mut speed = step.resources().get::<Controls>().unwrap().0;
            for event in step.input() {
                match event.input {
                    Input::Key {
                        key: PhysicalKey::Code(KeyCode::ArrowRight),
                        pressed,
                        ..
                    } => speed = i32::from(pressed),
                    Input::Key {
                        key: PhysicalKey::Code(KeyCode::ArrowLeft),
                        pressed,
                        ..
                    } => speed = -i32::from(pressed),
                    Input::Touch {
                        phase: TouchPhase::Started,
                        x,
                        y,
                        ..
                    } => {
                        commands.insert(actors[0], Destination(x as i32, y as i32));
                    }
                    _ => {}
                }
            }
            step.resources_mut().get_mut::<Controls>().unwrap().0 = speed;
            world
                .query::<(&mut Position, &mut Velocity)>()
                .for_each(|_, (position, velocity)| {
                    velocity.0 = speed;
                    position.0 += velocity.0;
                });
        });
    }
}

fn game(duration: u64) -> Simulation {
    App::new()
        .with_fixed_step(FixedStep::new(
            Duration::from_millis(duration),
            NonZeroU32::new(4).unwrap(),
        ))
        .add_plugin(Game)
        .into_simulation()
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    count: u32,
    actors: Vec<(Entity, Position, Velocity)>,
    destinations: Vec<(Entity, Destination)>,
}

fn snapshot(world: &World) -> Snapshot {
    let mut actors = Vec::new();
    let mut destinations = Vec::new();
    world
        .query::<(&Position, &Velocity)>()
        .for_each(|entity, (position, velocity)| {
            actors.push((entity, *position, *velocity));
        });
    world
        .query::<&Destination>()
        .for_each(|entity, destination| {
            destinations.push((entity, *destination));
        });
    Snapshot {
        count: world.entity_count(),
        actors,
        destinations,
    }
}

#[test]
fn serialized_input_replays_to_identical_world_state_without_frame_timing() {
    let mut windows = Vec::new();
    Commands::default().create_windows(&[WindowDesc::default()], &mut windows);
    let event = |time, input| InputEvent {
        window: windows[0],
        timestamp: InputTimestamp::Received(Duration::from_millis(time)),
        input,
    };
    let key = |code, pressed| Input::Key {
        key: PhysicalKey::Code(code),
        pressed,
        repeat: false,
    };

    let mut original = game(10);
    assert!(original.start_recording());
    assert!(!original.start_recording());
    original.queue_input([event(1, key(KeyCode::ArrowRight, true))]);
    assert_eq!(original.advance(Duration::from_millis(3)), 0);
    assert_eq!(original.advance(Duration::from_millis(28)), 3);
    original.queue_input([
        event(32, key(KeyCode::ArrowRight, false)),
        event(
            34,
            Input::Touch {
                id: 1,
                phase: TouchPhase::Started,
                x: 30.0,
                y: 42.0,
            },
        ),
    ]);
    assert_eq!(original.advance(Duration::from_millis(4)), 0);
    original.queue_input([event(41, key(KeyCode::ArrowLeft, true))]);
    assert_eq!(original.advance(Duration::from_millis(15)), 2);

    let recording = original.finish_recording().unwrap();
    assert_eq!(recording.step_count(), 5);
    let serialized = serde_json::to_string(&recording).unwrap();
    let recording = serde_json::from_str::<InputRecording>(&serialized).unwrap();
    let expected = snapshot(original.world());
    assert_eq!(expected.count, 2);
    assert_eq!(
        expected
            .actors
            .iter()
            .map(|(_, p, v)| (p.0, v.0))
            .collect::<Vec<_>>(),
        [(11, -1), (1, -1)]
    );
    assert_eq!(expected.destinations.len(), 1);
    assert_eq!(expected.destinations[0].1, Destination(30, 42));

    let mut replay = game(10);
    replay.replay(&recording).unwrap();
    assert_eq!(snapshot(replay.world()), expected);
    assert_eq!(replay.resources().get::<Controls>().unwrap().0, -1);
    assert_eq!(
        replay.replay(&recording),
        Err(ReplayError::StartStepMismatch {
            expected: 0,
            actual: 5
        })
    );
}

#[test]
fn replay_validates_its_version_and_step_duration_before_running_systems() {
    let mut original = game(10);
    original.start_recording();
    original.advance(Duration::from_millis(10));
    let recording = original.finish_recording().unwrap();
    let mut wrong_duration = game(12);
    let initial = snapshot(wrong_duration.world());
    assert_eq!(
        wrong_duration.replay(&recording),
        Err(ReplayError::StepDurationMismatch)
    );
    assert_eq!(snapshot(wrong_duration.world()), initial);

    let mut encoded = serde_json::to_value(recording).unwrap();
    encoded["version"] = 99.into();
    let unknown = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        game(10).replay(&unknown),
        Err(ReplayError::UnsupportedVersion(99))
    );
}
