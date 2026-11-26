#![cfg(feature = "physics")]

use std::num::NonZeroU32;
use std::time::Duration;

use engine_app::core::glam::{Quat, Vec3};
use engine_app::ecs::Entity;
use engine_app::physics::{
    BodyKind, Collider, ColliderShape, ContactEvent, KinematicMotion, PhysicsPlugin, PhysicsPose,
    PhysicsWorld, RigidBody,
};
use engine_app::{App, FixedStep};

const STEP: Duration = Duration::from_millis(10);

fn app() -> App {
    App::new().with_fixed_step(FixedStep::new(STEP, NonZeroU32::new(8).unwrap()))
}

fn at_y(y: f32) -> PhysicsPose {
    PhysicsPose {
        translation: Vec3::Y * y,
        ..PhysicsPose::default()
    }
}

fn floor() -> Collider {
    Collider::new(
        ColliderShape::Cuboid(Vec3::new(4.0, 0.5, 4.0)),
        at_y(-0.5),
        false,
    )
}

fn sphere() -> Collider {
    Collider::new(ColliderShape::Sphere(0.5), at_y(0.0), false)
}

fn contact(a: Entity, b: Entity, started: bool, sensor: bool) -> ContactEvent {
    let mut entities = [a, b];
    entities.sort_unstable();
    ContactEvent {
        entities,
        started,
        sensor,
    }
}

#[derive(Debug, Default, PartialEq)]
struct Trace {
    poses: Vec<PhysicsPose>,
    events: Vec<ContactEvent>,
}

fn run(frame: Duration) -> Trace {
    let mut app = app();
    let world = app.world_mut();
    let ground = world.spawn_empty();
    world.insert(ground, floor()).unwrap();
    let trigger = world.spawn_empty();
    world
        .insert(
            trigger,
            Collider::new(
                ColliderShape::Cuboid(Vec3::new(4.0, 0.1, 4.0)),
                at_y(2.0),
                true,
            ),
        )
        .unwrap();
    let ball = world.spawn_empty();
    world.insert(ball, sphere()).unwrap();
    world
        .insert(ball, RigidBody::new(BodyKind::Dynamic, at_y(4.0)))
        .unwrap();
    let mut app = app.add_plugin(PhysicsPlugin::default());
    app.insert_resource(Trace::default());
    let schedule = app.schedule_mut();
    let after_physics = schedule.add_stage();
    schedule.add_system(after_physics, move |world, _, context, _| {
        let resources = context.resources_mut();
        let events = resources.get::<PhysicsWorld>().unwrap().contacts().to_vec();
        let trace = resources.get_mut::<Trace>().unwrap();
        trace.poses.push(*world.get::<PhysicsPose>(ball).unwrap());
        trace.events.extend(events);
    });
    let mut simulation = app.into_simulation();
    for _ in 0..(Duration::from_secs(2).as_millis() / frame.as_millis()) {
        simulation.advance(frame);
    }
    let trace = simulation.resources().get::<Trace>().unwrap();
    assert_eq!(trace.poses.len(), 200);
    assert!(trace.poses[0].translation.y < 4.0);
    let landing_height = trace.poses.last().unwrap().translation.y;
    assert!(
        (landing_height - 0.5).abs() < 0.01,
        "height {landing_height}"
    );
    assert_eq!(
        trace.events,
        [
            contact(trigger, ball, true, true),
            contact(trigger, ball, false, true),
            contact(ground, ball, true, false),
        ]
    );
    Trace {
        poses: trace.poses.clone(),
        events: trace.events.clone(),
    }
}

#[test]
fn falling_landing_and_sensor_events_match_at_two_frame_rates() {
    assert_eq!(run(STEP), run(STEP * 2));
}

#[test]
fn physics_applies_commands_replacements_and_despawns_at_fixed_boundaries() {
    let mut app = app().add_plugin(PhysicsPlugin::default());
    let ground = app.world_mut().spawn_empty();
    let ball = app.world_mut().spawn_empty();
    app.add_system(move |_, commands, context, _| {
        if context.index() == 1 {
            commands.insert(ground, floor());
            commands.insert(ball, RigidBody::new(BodyKind::Dynamic, at_y(0.5)));
            commands.insert(ball, sphere());
        }
    });
    let mut simulation = app.into_simulation();
    assert_eq!(simulation.advance(STEP / 2), 0);
    assert!(simulation.world().get::<PhysicsPose>(ball).is_none());
    assert_eq!(simulation.advance(STEP / 2), 1);
    assert_eq!(
        simulation
            .resources()
            .get::<PhysicsWorld>()
            .unwrap()
            .contacts(),
        [contact(ground, ball, true, false)]
    );

    let world = simulation.world_mut();
    assert!(world.despawn(ground));
    let replacement = world.spawn_empty();
    assert_eq!(replacement.index(), ground.index());
    assert_ne!(replacement, ground);
    simulation.advance(STEP);
    assert_eq!(
        simulation
            .resources()
            .get::<PhysicsWorld>()
            .unwrap()
            .contacts(),
        [contact(ground, ball, false, false)]
    );
    for _ in 0..50 {
        simulation.advance(STEP);
    }
    assert!(
        simulation
            .world()
            .get::<PhysicsPose>(ball)
            .unwrap()
            .translation
            .y
            < 0.0
    );
    assert!(
        simulation
            .resources()
            .get::<PhysicsWorld>()
            .unwrap()
            .contacts()
            .is_empty()
    );

    let world = simulation.world_mut();
    world.remove::<RigidBody>(ball).unwrap();
    let fixed_pose = PhysicsPose {
        translation: Vec3::new(0.0, 5.0, 0.0),
        rotation: Quat::from_rotation_z(0.5),
    };
    world
        .insert(ball, RigidBody::new(BodyKind::Fixed, fixed_pose))
        .unwrap();
    simulation.advance(STEP);
    assert_eq!(
        *simulation.world().get::<PhysicsPose>(ball).unwrap(),
        fixed_pose
    );

    let world = simulation.world_mut();
    world.remove::<Collider>(ball).unwrap();
    world.insert(replacement, floor()).unwrap();
    world.remove::<RigidBody>(ball).unwrap();
    world
        .insert(ball, RigidBody::new(BodyKind::Dynamic, at_y(0.5)))
        .unwrap();
    simulation.advance(STEP);
    assert!(
        simulation
            .resources()
            .get::<PhysicsWorld>()
            .unwrap()
            .contacts()
            .is_empty()
    );
    simulation.world_mut().insert(ball, sphere()).unwrap();
    simulation.advance(STEP);
    assert_eq!(
        simulation
            .resources()
            .get::<PhysicsWorld>()
            .unwrap()
            .contacts(),
        [contact(replacement, ball, true, false)]
    );

    simulation.world_mut().remove::<RigidBody>(ball).unwrap();
    simulation.advance(STEP);
    assert!(simulation.world().get::<PhysicsPose>(ball).is_none());
    assert_eq!(
        simulation
            .resources()
            .get::<PhysicsWorld>()
            .unwrap()
            .contacts(),
        [contact(replacement, ball, false, false)]
    );
}

#[test]
fn kinematic_move_sees_walls_added_and_removed_before_its_step() {
    let mut app = app().add_plugin(PhysicsPlugin::default());
    let world = app.world_mut();
    let wall = world.spawn_empty();
    let sensor = world.spawn_empty();
    let wall_pose = PhysicsPose {
        translation: Vec3::X,
        ..PhysicsPose::default()
    };
    let wall_shape = ColliderShape::Cuboid(Vec3::new(0.1, 1.0, 1.0));
    world
        .insert(sensor, Collider::new(wall_shape, wall_pose, true))
        .unwrap();
    let pawn = world.spawn_empty();
    world
        .insert(
            pawn,
            RigidBody::new(BodyKind::Kinematic, PhysicsPose::default()),
        )
        .unwrap();
    world.insert(pawn, sphere()).unwrap();
    world.insert(pawn, KinematicMotion::default()).unwrap();
    app.add_system(move |world, commands, step, _| {
        if step.index() == 1 {
            commands.insert(wall, Collider::new(wall_shape, wall_pose, false));
        }
        if step.index() <= 2 {
            world
                .query::<&mut KinematicMotion>()
                .get_mut(pawn)
                .unwrap()
                .request_translation(Vec3::X * 2.0);
        }
    });

    let mut simulation = app.into_simulation();
    simulation.advance(STEP);
    let stopped = simulation
        .world()
        .get::<PhysicsPose>(pawn)
        .unwrap()
        .translation
        .x;
    assert!((0.35..0.41).contains(&stopped), "stopped at {stopped}");

    simulation.world_mut().remove::<Collider>(wall).unwrap();
    simulation.advance(STEP);
    let passed = simulation
        .world()
        .get::<PhysicsPose>(pawn)
        .unwrap()
        .translation
        .x;
    assert!((passed - stopped - 2.0).abs() < 0.001);

    simulation.advance(STEP);
    let world = simulation.world();
    assert_eq!(
        world.get::<PhysicsPose>(pawn).unwrap().translation.x,
        passed
    );
    assert!(
        world
            .get::<KinematicMotion>(pawn)
            .unwrap()
            .result()
            .is_none()
    );
}
