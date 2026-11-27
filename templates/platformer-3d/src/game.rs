use std::{cell::RefCell, error::Error, rc::Rc};

use engine_app::core::glam::{Quat, Vec3};
use engine_app::physics::{
    BodyKind, Collider, ColliderShape, KinematicMotion, PhysicsInterpolation, PhysicsPose,
    PhysicsWorld, RigidBody,
};
use engine_app::platform::{Input, KeyCode, PhysicalKey, WindowDesc};
use engine_app::scene::{Camera, Mesh, MeshRegistry, Renderable, Transform};
use engine_app::{ActionBinding, App, InputActions, InputContext, StepContext};
use engine_ecs::{Commands, Component, Entity, World};
use engine_ui::input::InputResult;

use crate::level::{self, Checkpoint, Coin};
use crate::tuning::Tuning;
use crate::ui::{Interaction, MenuAction, TouchButton};

const PLAYER_RADIUS: f32 = 0.44;
const RESPAWN_HEIGHT: f32 = -4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Left,
    Right,
    Jump,
}

#[derive(Default)]
struct Menu {
    paused: bool,
    restart: bool,
}

#[derive(Debug, Component)]
pub struct Run {
    pub coins: usize,
    pub total: usize,
    pub checkpoint: Option<Entity>,
    pub paused: bool,
    vertical_speed: f32,
    pub respawn_pending: bool,
}

pub struct Game {
    pub app: App,
    pub meshes: MeshRegistry,
    pub player: Entity,
    pub interaction: Rc<RefCell<Interaction>>,
}

impl Game {
    pub fn build() -> Result<Self, Box<dyn Error>> {
        let mut meshes = MeshRegistry::default();
        let level::Level {
            mut world,
            start,
            coin_total,
        } = level::load(&mut meshes)?;
        let player = world.spawn_empty();
        world.insert(player, player_body(start)).unwrap();
        world
            .insert(
                player,
                Collider::new(
                    ColliderShape::Sphere(PLAYER_RADIUS),
                    PhysicsPose::default(),
                    false,
                ),
            )
            .unwrap();
        world.insert(player, KinematicMotion::default()).unwrap();
        world
            .insert(player, PhysicsInterpolation::default())
            .unwrap();
        world
            .insert(
                player,
                Transform {
                    translation: start,
                    scale: Vec3::splat(PLAYER_RADIUS * 2.0),
                    ..Transform::default()
                },
            )
            .unwrap();
        world
            .insert(
                player,
                Renderable {
                    mesh: meshes.insert(Mesh::sphere()),
                    color: [0.11, 0.85, 0.74, 1.0],
                },
            )
            .unwrap();
        world
            .insert(
                player,
                Run {
                    coins: 0,
                    total: coin_total,
                    checkpoint: None,
                    paused: false,
                    vertical_speed: 0.0,
                    respawn_pending: false,
                },
            )
            .unwrap();

        let camera = world.spawn_empty();
        world.insert(camera, Camera::default()).unwrap();
        world.insert(camera, camera_at(start.x)).unwrap();
        let physics = PhysicsWorld::install(&mut world, Vec3::new(0.0, -9.81, 0.0));
        let interaction = Rc::new(RefCell::new(Interaction::default()));
        let mut app = App::new().with_window(WindowDesc::new("Platformer 3D").with_size(1040, 640));
        *app.world_mut() = world;
        app.insert_resource(physics);
        app.insert_resource(Menu::default());
        app.world_mut().resources_mut().insert(Tuning::bundled());
        install_input(&mut app, Rc::clone(&interaction));
        app.add_system(move |world, commands, step, _| {
            update(world, commands, step, player, start)
        });
        let schedule = app.schedule_mut();
        let physics = schedule.add_stage();
        schedule.add_system(physics, move |world, commands, step, _| {
            if step.resources().get::<Menu>().unwrap().paused {
                world
                    .query::<&mut PhysicsInterpolation>()
                    .get_mut(player)
                    .unwrap()
                    .snap_to_latest();
            } else {
                step.step_physics(world, commands);
            }
        });
        let contact_stage = schedule.add_stage();
        schedule.add_system(contact_stage, move |world, commands, step, _| {
            after_physics(world, commands, step, player, camera);
        });
        Ok(Self {
            app,
            meshes,
            player,
            interaction,
        })
    }
}

fn install_input(app: &mut App, interaction: Rc<RefCell<Interaction>>) {
    let key = |code| ActionBinding::Key(PhysicalKey::Code(code));
    let mut actions = InputActions::default();
    actions.push_context(
        InputContext::default()
            .bind(key(KeyCode::KeyA), Action::Left)
            .bind(key(KeyCode::ArrowLeft), Action::Left)
            .bind(key(KeyCode::KeyD), Action::Right)
            .bind(key(KeyCode::ArrowRight), Action::Right)
            .bind(key(KeyCode::Space), Action::Jump)
            .bind(TouchButton::Left.binding(), Action::Left)
            .bind(TouchButton::Right.binding(), Action::Right)
            .bind(TouchButton::Jump.binding(), Action::Jump),
    );
    app.set_input_actions(actions, move |event, actions, resources| {
        let mut interaction = interaction.borrow_mut();
        let menu = resources.get_mut::<Menu>().unwrap();
        let response = interaction.route(event, menu.paused);
        let toggle = matches!(
            event.input,
            Input::Key {
                key: PhysicalKey::Code(KeyCode::Escape | KeyCode::KeyP),
                pressed: true,
                repeat: false,
            }
        );
        let next = match response {
            InputResult::Action(MenuAction::Pause) => Some(true),
            InputResult::Action(MenuAction::Resume) => Some(false),
            InputResult::Action(MenuAction::Restart) => {
                menu.restart = true;
                Some(false)
            }
            _ if toggle => Some(!menu.paused),
            _ => None,
        };
        if let Some(paused) = next.filter(|&paused| paused != menu.paused) {
            #[cfg(debug_assertions)]
            eprintln!("platformer: paused={paused}");
            menu.paused = paused;
            interaction.reset();
            if paused {
                actions.push_context(InputContext::default().modal());
            } else {
                actions.pop_context();
            }
        }
        if !menu.paused
            && !response.consumed()
            && let Some(button) = interaction.touch_button(event)
        {
            actions.press_touch_button(event, button as u16);
        }
        response.consumed() || toggle
    });
}

fn player_body(position: Vec3) -> RigidBody {
    RigidBody::new(
        BodyKind::Kinematic,
        PhysicsPose {
            translation: position,
            ..PhysicsPose::default()
        },
    )
}

fn camera_at(x: f32) -> Transform {
    Transform {
        translation: Vec3::new(x + 1.6, 3.9, 9.2),
        rotation: Quat::from_rotation_x(-0.29),
        ..Transform::default()
    }
}

fn update(
    world: &World,
    commands: &mut Commands,
    step: &mut StepContext,
    player: Entity,
    start: Vec3,
) {
    let tuning = *world.resources().get::<Tuning>().unwrap();
    let menu = step.resources_mut().get_mut::<Menu>().unwrap();
    let restart = std::mem::take(&mut menu.restart);
    let mut runs = world.query::<&mut Run>();
    let run = runs.get_mut(player).unwrap();
    run.paused = menu.paused;
    if restart {
        restore_coins(world, commands);
        if let Some(checkpoint) = run.checkpoint.take() {
            let marker = world.get::<Checkpoint>(checkpoint).unwrap();
            commands.insert(checkpoint, marker.inactive);
        }
        run.coins = 0;
        respawn(commands, player, run, start);
        return;
    }
    if run.paused {
        return;
    }
    if run.respawn_pending {
        let position = run.checkpoint.map_or(start, |entity| {
            world.get::<Checkpoint>(entity).unwrap().respawn
        });
        #[cfg(debug_assertions)]
        eprintln!(
            "platformer: respawn x={:.2}, coins={}",
            position.x, run.coins
        );
        respawn(commands, player, run, position);
        return;
    }
    let actions = step.actions::<Action>().unwrap();
    let movement =
        i8::from(actions.state(Action::Right).held) - i8::from(actions.state(Action::Left).held);
    let mut motions = world.query::<&mut KinematicMotion>();
    let motion = motions.get_mut(player).unwrap();
    if run.vertical_speed <= 0.0 && motion.result().is_some_and(|movement| movement.grounded) {
        run.vertical_speed = if actions.state(Action::Jump).pressed {
            #[cfg(debug_assertions)]
            eprintln!(
                "platformer: jump x={:.2}, direction={movement}, speed={}",
                world.get::<PhysicsPose>(player).unwrap().translation.x,
                tuning.jump_speed
            );
            tuning.jump_speed
        } else {
            0.0
        };
    }
    let seconds = step.delta().as_secs_f32();
    run.vertical_speed = (run.vertical_speed - tuning.fall_acceleration * seconds).max(-24.0);
    motion.request_translation(
        Vec3::new(
            f32::from(movement) * tuning.move_speed,
            run.vertical_speed,
            0.0,
        ) * seconds,
    );
}

fn restore_coins(world: &World, commands: &mut Commands) {
    world.query::<&mut Coin>().for_each(|entity, coin| {
        if std::mem::take(&mut coin.collected) {
            commands.insert(entity, coin.sensor);
            commands.insert(entity, coin.visual);
        }
    });
}

fn respawn(commands: &mut Commands, player: Entity, run: &mut Run, position: Vec3) {
    run.vertical_speed = 0.0;
    run.respawn_pending = false;
    commands.remove::<RigidBody>(player);
    commands.insert(player, player_body(position));
    commands.insert(player, KinematicMotion::default());
}

fn after_physics(
    world: &World,
    commands: &mut Commands,
    step: &StepContext,
    player: Entity,
    camera: Entity,
) {
    if step.resources().get::<Menu>().unwrap().paused {
        return;
    }
    let position = world.get::<PhysicsPose>(player).unwrap().translation;
    let mut runs = world.query::<&mut Run>();
    let run = runs.get_mut(player).unwrap();
    run.respawn_pending = position.y < RESPAWN_HEIGHT;
    let next_camera = camera_at(position.x);
    if world
        .get::<Transform>(camera)
        .is_none_or(|current| *current != next_camera)
    {
        commands.insert(camera, next_camera);
    }
    let mut coins = world.query::<&mut Coin>();
    let physics = step.resources().get::<PhysicsWorld>().unwrap();
    for contact in physics
        .contacts()
        .iter()
        .filter(|contact| contact.started && contact.sensor)
    {
        let [one, two] = contact.entities;
        let other = if one == player {
            two
        } else if two == player {
            one
        } else {
            continue;
        };
        if let Some(coin) = coins.get_mut(other)
            && !std::mem::replace(&mut coin.collected, true)
        {
            run.coins += 1;
            #[cfg(debug_assertions)]
            eprintln!("platformer: coin {}/{}", run.coins, run.total);
            commands.remove::<Renderable>(other);
            commands.remove::<Collider>(other);
        }
        if let Some(checkpoint) = world.get::<Checkpoint>(other)
            && run.checkpoint != Some(other)
        {
            #[cfg(debug_assertions)]
            eprintln!("platformer: checkpoint x={:.2}", checkpoint.respawn.x);
            if let Some(previous) = run.checkpoint.replace(other) {
                commands.insert(
                    previous,
                    world.get::<Checkpoint>(previous).unwrap().inactive,
                );
            }
            commands.insert(
                other,
                Renderable {
                    color: level::CHECKPOINT_ACTIVE,
                    ..checkpoint.inactive
                },
            );
        }
    }
}
