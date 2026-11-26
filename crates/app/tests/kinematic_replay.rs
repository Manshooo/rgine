#![cfg(feature = "physics")]

use std::time::Duration;

use engine_app::core::glam::Vec3;
use engine_app::physics::{
    BodyKind, Collider, ColliderShape, KinematicMotion, PhysicsPlugin, PhysicsPose, RigidBody,
};
use engine_app::platform::{
    Commands, Input, InputEvent, InputTimestamp, KeyCode, PhysicalKey, WindowDesc,
};
use engine_app::{ActionBinding, App, InputActions, InputContext, InputRecording, Simulation};

const STEP: Duration = Duration::from_nanos(16_666_667);
const JUMP: usize = 46;
const AIRBORNE_PRESS: usize = 58;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Move,
    Jump,
}

#[derive(Debug, PartialEq)]
struct Sample {
    position: Vec3,
    grounded: bool,
}

#[derive(Default)]
struct Trace(Vec<Sample>);

fn fixture() -> Simulation {
    let mut app = App::new().add_plugin(PhysicsPlugin::default());
    let world = app.world_mut();
    for (x, width) in [(-2.0, 6.0), (5.0, 5.0)] {
        let platform = world.spawn_empty();
        let collider = Collider::new(
            ColliderShape::Cuboid(Vec3::new(width * 0.5, 0.25, 1.5)),
            PhysicsPose {
                translation: Vec3::new(x, -0.25, 0.0),
                ..PhysicsPose::default()
            },
            false,
        );
        world.insert(platform, collider).unwrap();
    }
    let character = world.spawn_empty();
    let body = RigidBody::new(
        BodyKind::Kinematic,
        PhysicsPose {
            translation: Vec3::new(-1.5, 0.55, 0.0),
            ..PhysicsPose::default()
        },
    );
    let collider = Collider::new(ColliderShape::Sphere(0.5), PhysicsPose::default(), false);
    world.insert(character, body).unwrap();
    world.insert(character, collider).unwrap();
    world.insert(character, KinematicMotion::default()).unwrap();

    let mut actions = InputActions::default();
    actions.push_context(
        InputContext::default()
            .bind(
                ActionBinding::Key(PhysicalKey::Code(KeyCode::KeyD)),
                Action::Move,
            )
            .bind(
                ActionBinding::Key(PhysicalKey::Code(KeyCode::Space)),
                Action::Jump,
            ),
    );
    app.set_input_actions(actions, |_, _, _| false);
    let mut vertical_speed = 0.0;
    app.add_system(move |world, _, step, _| {
        let actions = step.actions::<Action>().unwrap();
        let mut motions = world.query::<&mut KinematicMotion>();
        let motion = motions.get_mut(character).unwrap();
        if motion.result().is_some_and(|last_move| last_move.grounded) && vertical_speed <= 0.0 {
            vertical_speed = if actions.state(Action::Jump).pressed {
                5.5
            } else {
                0.0
            };
        }
        let seconds = step.delta().as_secs_f32();
        vertical_speed -= 18.0 * seconds;
        let horizontal_speed = if actions.state(Action::Move).held {
            5.0
        } else {
            0.0
        };
        motion.request_translation(Vec3::new(horizontal_speed, vertical_speed, 0.0) * seconds);
    });

    app.insert_resource(Trace::default());
    let schedule = app.schedule_mut();
    let sample = schedule.add_stage();
    schedule.add_system(sample, move |world, _, step, _| {
        let pose = world.get::<PhysicsPose>(character).unwrap();
        let motion = world.get::<KinematicMotion>(character).unwrap();
        step.resources_mut()
            .get_mut::<Trace>()
            .unwrap()
            .0
            .push(Sample {
                position: pose.translation,
                grounded: motion.result().unwrap().grounded,
            });
    });
    app.into_simulation()
}

fn record(press_while_airborne: bool) -> (Simulation, InputRecording) {
    let mut windows = Vec::new();
    Commands::default().create_windows(&[WindowDesc::default()], &mut windows);
    let window = windows[0];
    let mut simulation = fixture();
    assert!(simulation.start_recording());
    let mut inputs = vec![
        (20, KeyCode::KeyD, true),
        (JUMP, KeyCode::Space, true),
        (JUMP + 1, KeyCode::Space, false),
        (101, KeyCode::KeyD, false),
    ];
    if press_while_airborne {
        inputs.extend([
            (AIRBORNE_PRESS, KeyCode::Space, true),
            (AIRBORNE_PRESS + 1, KeyCode::Space, false),
        ]);
    }
    for tick in 0..130 {
        for &(_, key, pressed) in inputs.iter().filter(|(step, _, _)| *step == tick) {
            simulation.queue_input([InputEvent {
                window,
                timestamp: InputTimestamp::Received(Duration::ZERO),
                input: Input::Key {
                    key: PhysicalKey::Code(key),
                    pressed,
                    repeat: false,
                },
            }]);
        }
        assert_eq!(simulation.advance(STEP), 1);
    }
    let recording = simulation.finish_recording().unwrap();
    (simulation, recording)
}

#[test]
fn fixed_step_input_jumps_once_and_replays_the_same_trajectory() {
    let (live, recording) = record(true);
    let trace = &live.resources().get::<Trace>().unwrap().0;
    let takeoff = &trace[JUMP - 1];
    assert!(takeoff.grounded && takeoff.position.x > 0.4);
    assert!(trace[JUMP].position.y > takeoff.position.y);
    assert!(!trace[AIRBORNE_PRESS - 1].grounded);
    let landing = trace[JUMP..].iter().find(|sample| sample.grounded).unwrap();
    assert!(landing.position.x > 2.5, "landed at {landing:?}");
    assert!(trace[129].grounded);
    assert!(trace[105].position.distance(trace[129].position) < 0.001);

    let (single_jump, _) = record(false);
    assert_eq!(&single_jump.resources().get::<Trace>().unwrap().0, trace);

    let serialized = serde_json::to_vec(&recording).unwrap();
    let restored = serde_json::from_slice(&serialized).unwrap();
    let mut replay = fixture();
    replay.replay(&restored).unwrap();
    assert_eq!(&replay.resources().get::<Trace>().unwrap().0, trace);
}
