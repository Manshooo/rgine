//! A top-down physics and input-context exercise.

mod profile;
mod tuning;
mod visuals;

#[cfg(test)]
mod tests;

use engine_app::core::glam::{Vec3, Vec3Swizzles};
use engine_app::ecs::{Commands, Component, Entity, World};
use engine_app::physics::{
    BodyKind, Collider, ColliderShape, KinematicMotion, PhysicsInterpolation, PhysicsPose,
    PhysicsWorld, RigidBody,
};
use engine_app::platform::{Input, KeyCode, PhysicalKey, WindowDesc};
use engine_app::scene::{MeshKey, MeshRegistry, Renderable, Transform};
use engine_app::{ActionBinding, App, InputActions, InputContext, StepContext};

use tuning::Tuning;

const TARGET_RADIUS: f32 = 2.7;
const PADDLE_RADIUS: f32 = 0.68;
const PADDLE_HEIGHT: f32 = 0.24;
const BODY_Y: f32 = 0.34;
const BALL_RADIUS: f32 = 0.32;
const PADDLE_COLOR: [f32; 4] = [0.88, 0.39, 0.075, 1.0];
const STRIKE_COLOR: [f32; 4] = [1.0, 0.86, 0.31, 1.0];
const BALL_COLOR: [f32; 4] = [0.19, 0.67, 0.92, 1.0];
const RETURN_COLOR: [f32; 4] = [0.25, 0.83, 0.38, 1.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Left,
    Right,
    Up,
    Down,
    Strike,
    MenuUp,
    MenuDown,
    Confirm,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MenuItem {
    #[default]
    Resume,
    Restart,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Arena {
    tick: u64,
    served: usize,
    hits: u32,
    misses: u32,
    paused: bool,
    step_physics: bool,
    target_live: bool,
    menu: MenuItem,
    strike_started: u64,
    strike_until: u64,
    recovery_until: u64,
    strike_direction: Vec3,
}

impl Arena {
    fn striking(&self) -> bool {
        self.tick < self.strike_until
    }
}

#[derive(Component)]
struct Incoming {
    served_at: u64,
    returned: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const CLEAR: [f64; 4] = [0.008, 0.016, 0.028, 1.0];
    engine_app::core::init_logging();
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--profile")) {
        return profile::run(arguments, CLEAR);
    }
    let (app, meshes, _) = arena_app();
    #[cfg(feature = "hot-reload")]
    if let Some(path) = std::env::args_os().nth(1).map(std::path::PathBuf::from) {
        use engine_app::hot_reload::RonReload;

        let (mut reload, tuning) = RonReload::load(path.clone(), Tuning::validate)?;
        let mut app = app;
        app.world_mut().resources_mut().insert(tuning);
        app.run_scene_with_refresh(meshes, CLEAR, move |world, _| {
            let active = world.resources_mut().get_mut::<Tuning>().unwrap();
            match reload.poll(active) {
                Ok(true) => {
                    eprintln!("hot reload: applied {}", path.display());
                    true
                }
                Ok(false) => false,
                Err(error) => {
                    eprintln!("hot reload: {error}");
                    false
                }
            }
        })?;
        return Ok(());
    }
    app.run_scene(meshes, CLEAR)?;
    Ok(())
}

fn arena_app() -> (App, MeshRegistry, Entity) {
    arena_with_hud(visuals::HudMode::Original)
}

fn arena_with_hud(mode: visuals::HudMode) -> (App, MeshRegistry, Entity) {
    let mut app = App::new().with_window(WindowDesc::new("Deflection Arena").with_size(1120, 720));
    let (meshes, cylinder, ball, mut overlay) = visuals::scene(app.world_mut(), mode);
    let world = app.world_mut();
    world.resources_mut().insert(Tuning::bundled());
    let paddle = world.spawn_empty();
    world.insert(paddle, paddle_body()).unwrap();
    world
        .insert(paddle, PhysicsInterpolation::default())
        .unwrap();
    world
        .insert(
            paddle,
            Collider::new(
                ColliderShape::CylinderY {
                    radius: PADDLE_RADIUS,
                    half_height: PADDLE_HEIGHT * 0.5,
                },
                PhysicsPose::default(),
                false,
            )
            .with_restitution(0.85),
        )
        .unwrap();
    world.insert(paddle, KinematicMotion::default()).unwrap();
    world
        .insert(
            paddle,
            Transform {
                translation: Vec3::Y * BODY_Y,
                scale: Vec3::new(PADDLE_RADIUS * 2.0, PADDLE_HEIGHT, PADDLE_RADIUS * 2.0),
                ..Transform::default()
            },
        )
        .unwrap();
    world
        .insert(
            paddle,
            Renderable {
                mesh: cylinder,
                color: PADDLE_COLOR,
            },
        )
        .unwrap();

    // The example controls when to step physics so a modal pause freezes
    // velocities and flight time as well as the player's input.
    let physics = PhysicsWorld::install(world, Vec3::ZERO);
    app.insert_resource(physics);
    app.insert_resource(Arena::default());
    install_input(&mut app);
    app.add_system(move |world, commands, step, _| update(world, commands, step, paddle, ball));
    let schedule = app.schedule_mut();
    let physics = schedule.add_stage();
    schedule.add_system(physics, |world, commands, step, _| {
        if step.resources().get::<Arena>().unwrap().step_physics {
            step.step_physics(world, commands);
        } else {
            world
                .query::<&mut PhysicsInterpolation>()
                .for_each(|_, samples| samples.snap_to_latest());
        }
    });
    let presentation = schedule.add_stage();
    schedule.add_system(presentation, move |world, commands, step, _| {
        publish(world, commands, step, paddle);
        overlay.update(world, commands, step.resources().get::<Arena>().unwrap());
    });
    (app, meshes, paddle)
}

fn install_input(app: &mut App) {
    use Action::*;
    use KeyCode::*;
    let mut actions = InputActions::default();
    actions.push_context(bindings(&[
        (KeyA, Left),
        (ArrowLeft, Left),
        (KeyD, Right),
        (ArrowRight, Right),
        (KeyW, Up),
        (ArrowUp, Up),
        (KeyS, Down),
        (ArrowDown, Down),
        (Space, Strike),
    ]));
    app.set_input_actions(actions, |event, actions, resources| {
        if !matches!(
            event.input,
            Input::Key {
                key: PhysicalKey::Code(Escape),
                pressed: true,
                repeat: false,
            }
        ) {
            return false;
        }
        let arena = resources.get_mut::<Arena>().unwrap();
        arena.paused = !arena.paused;
        if arena.paused {
            arena.menu = MenuItem::Resume;
            actions.push_context(
                bindings(&[
                    (KeyW, MenuUp),
                    (ArrowUp, MenuUp),
                    (KeyS, MenuDown),
                    (ArrowDown, MenuDown),
                    (Space, Confirm),
                    (Enter, Confirm),
                ])
                .modal(),
            );
        } else {
            actions.pop_context();
        }
        true
    });
}

fn bindings(keys: &[(KeyCode, Action)]) -> InputContext<Action> {
    keys.iter()
        .fold(InputContext::default(), |context, &(key, action)| {
            context.bind(ActionBinding::Key(PhysicalKey::Code(key)), action)
        })
}

fn update(
    world: &World,
    commands: &mut Commands,
    step: &mut StepContext,
    paddle: Entity,
    ball: MeshKey,
) {
    let actions = step.actions::<Action>().unwrap();
    let held = |action| f32::from(u8::from(actions.state(action).held));
    let direction = Vec3::new(
        held(Action::Right) - held(Action::Left),
        0.0,
        held(Action::Down) - held(Action::Up),
    )
    .clamp_length_max(1.0);
    let strike = actions.state(Action::Strike).pressed;
    let menu_up = actions.state(Action::MenuUp).pressed;
    let menu_down = actions.state(Action::MenuDown).pressed;
    let confirm = actions.state(Action::Confirm).pressed;
    let dt = step.delta().as_secs_f32();
    let arena = step.resources_mut().get_mut::<Arena>().unwrap();
    arena.step_physics = false;
    if arena.paused {
        if menu_up {
            arena.menu = MenuItem::Resume;
        }
        if menu_down {
            arena.menu = MenuItem::Restart;
        }
        if confirm {
            if arena.menu == MenuItem::Restart {
                *arena = Arena::default();
                arena.step_physics = true;
                commands.remove::<RigidBody>(paddle);
                commands.insert(paddle, paddle_body());
                world
                    .query::<&Incoming>()
                    .for_each(|entity, _| commands.despawn(entity));
            }
            arena.paused = false;
            step.actions_mut::<Action>().unwrap().pop_context();
        }
        return;
    }

    arena.step_physics = true;
    arena.tick += 1;
    if (arena.tick - 1).is_multiple_of(108) {
        serve(commands, ball, arena.tick, arena.served);
        arena.served += 1;
    }
    let position = world
        .get::<PhysicsPose>(paddle)
        .map_or(Vec3::Y * BODY_Y, |pose| pose.translation);
    let tuning = world.resources().get::<Tuning>().unwrap();
    if strike && arena.tick >= arena.recovery_until {
        arena.strike_direction = direction
            .try_normalize()
            .unwrap_or_else(|| aim(world, position));
        arena.strike_started = arena.tick;
        arena.strike_until = arena.tick + u64::from(tuning.transitions.strike_to_cooldown_ticks);
        arena.recovery_until =
            arena.strike_until + u64::from(tuning.transitions.cooldown_to_ready_ticks);
    }
    let velocity = if arena.striking() {
        let elapsed = arena.tick - arena.strike_started;
        let last_tick = (arena.strike_until - arena.strike_started - 1).max(1);
        let speed = tuning.strike_speed(elapsed as f32 / last_tick as f32);
        arena.strike_direction * speed
    } else {
        direction * tuning.move_speed
    };
    let desired = position + velocity * dt;
    let target = Vec3::new(
        desired.x.clamp(-7.1, 7.1),
        BODY_Y,
        desired.z.clamp(-4.05, 4.05),
    );
    world
        .query::<&mut KinematicMotion>()
        .get_mut(paddle)
        .unwrap()
        .request_direct_translation(target - position);
}

fn paddle_body() -> RigidBody {
    RigidBody::new(
        BodyKind::Kinematic,
        PhysicsPose {
            translation: Vec3::Y * BODY_Y,
            ..PhysicsPose::default()
        },
    )
}

fn aim(world: &World, paddle: Vec3) -> Vec3 {
    let mut nearest = Vec3::X;
    let mut distance = f32::INFINITY;
    world
        .query::<(&Incoming, &PhysicsPose)>()
        .for_each(|_, (ball, pose)| {
            let offset = (pose.translation - paddle) * Vec3::new(1.0, 0.0, 1.0);
            let squared = offset.length_squared();
            if !ball.returned && squared < distance && squared > 0.0 {
                nearest = offset.normalize();
                distance = squared;
            }
        });
    nearest
}

fn serve(commands: &mut Commands, mesh: MeshKey, tick: u64, serve: usize) {
    let ports = [
        (7.75, 0.0),
        (-2.9, -4.40),
        (-7.75, 1.1),
        (2.3, 4.40),
        (7.75, -3.7),
        (-6.7, 4.40),
    ];
    let (x, z) = ports[serve % ports.len()];
    let translation = Vec3::new(x, BODY_Y, z);
    let velocity = -Vec3::new(x, 0.0, z).normalize() * (5.0 + (serve % 3) as f32 * 0.5);
    let pose = PhysicsPose {
        translation,
        ..PhysicsPose::default()
    };
    commands
        .spawn_empty()
        .insert(RigidBody::new(BodyKind::Dynamic, pose).with_linear_velocity(velocity))
        .insert(PhysicsInterpolation::default())
        .insert(
            Collider::new(
                ColliderShape::Sphere(BALL_RADIUS),
                PhysicsPose::default(),
                false,
            )
            .with_restitution(0.85),
        )
        .insert(Incoming {
            served_at: tick,
            returned: false,
        })
        .insert(Renderable {
            mesh,
            color: BALL_COLOR,
        })
        .insert(Transform {
            translation,
            scale: Vec3::splat(BALL_RADIUS * 2.0),
            ..Transform::default()
        });
}

fn publish(world: &World, commands: &mut Commands, step: &mut StepContext, paddle: Entity) {
    let arena = step.resources().get::<Arena>().unwrap();
    let mut hits = 0;
    let mut misses = 0;
    let mut target_live = arena.target_live;
    if arena.step_physics {
        target_live = false;
        let contacts = step.resources().get::<PhysicsWorld>().unwrap().contacts();
        let mut balls = world.query::<(&mut Incoming, &PhysicsPose, &mut Renderable)>();
        balls.for_each(|entity, (ball, pose, renderable)| {
            let position = pose.translation;
            let in_zone = position.xz().length_squared() <= TARGET_RADIUS * TARGET_RADIUS;
            if arena.striking()
                && !ball.returned
                && in_zone
                && contacts.iter().any(|contact| {
                    contact.started
                        && !contact.sensor
                        && contact.entities.contains(&paddle)
                        && contact.entities.contains(&entity)
                })
            {
                ball.returned = true;
                renderable.color = RETURN_COLOR;
                hits += 1;
            }
            if position.x.abs() > 8.9 || position.z.abs() > 6.2 || arena.tick - ball.served_at > 270
            {
                misses += u32::from(!ball.returned);
                commands.despawn(entity);
            } else {
                target_live |= in_zone && !ball.returned;
            }
        });
    }
    world
        .query::<&mut Renderable>()
        .get_mut(paddle)
        .unwrap()
        .color = if arena.striking() {
        STRIKE_COLOR
    } else {
        PADDLE_COLOR
    };
    let arena = step.resources_mut().get_mut::<Arena>().unwrap();
    arena.target_live = target_live;
    arena.hits += hits;
    arena.misses += misses;
}
