use std::any::TypeId;

use engine_ecs::{Commands, Component, ComponentLifecycle, Entity, World};

#[derive(Component, Debug, Default)]
#[component(requires(Motor, Powered), on_add = machine_added)]
struct Machine;

#[derive(Component, Debug, Default)]
#[component(requires(Rotation, Machine), on_add = motor_added)]
struct Motor(u32);

#[derive(Component, Debug, Default)]
struct Rotation(u32);

#[derive(Component, Default)]
#[component(storage = "sparse")]
struct Powered;

#[derive(Component)]
struct Initialized;

fn machine_added(world: &World, entity: Entity, commands: &mut Commands) {
    assert!(world.get::<Machine>(entity).is_some());
    assert!(world.get::<Motor>(entity).is_some());
    assert!(world.get::<Rotation>(entity).is_some());
    assert!(world.get::<Powered>(entity).is_some());
    commands.insert(entity, Initialized);
}

fn motor_added(world: &World, entity: Entity, _: &mut Commands) {
    assert!(world.get::<Machine>(entity).is_some());
    assert!(world.get::<Powered>(entity).is_some());
}

#[test]
fn requirements_follow_cycles_once_and_preserve_preexisting_values() {
    let mut world = World::new();
    let entity = world.spawn_empty();
    world.insert(entity, Rotation(42)).unwrap();
    world.insert(entity, Machine).unwrap();

    assert_eq!(world.get::<Motor>(entity).unwrap().0, 0);
    assert_eq!(world.get::<Rotation>(entity).unwrap().0, 42);
    assert!(world.get::<Powered>(entity).is_some());
    assert!(world.get::<Initialized>(entity).is_none());
    let lifecycle = world
        .type_registry()
        .get::<Machine>()
        .unwrap()
        .metadata::<ComponentLifecycle>()
        .unwrap();
    assert_eq!(lifecycle.required[0].type_id(), TypeId::of::<Motor>());
    assert_eq!(lifecycle.required[1].type_id(), TypeId::of::<Powered>());
    assert!(lifecycle.on_add.is_some());

    let mut commands = Commands::default();
    commands.apply(&mut world);
    assert!(world.remove::<Initialized>(entity).is_some());

    world.get_mut::<Motor>(entity).unwrap().0 = 7;
    world.remove::<Rotation>(entity).unwrap();
    assert!(world.insert(entity, Machine).unwrap().is_some());
    assert_eq!(world.get::<Motor>(entity).unwrap().0, 7);
    assert_eq!(world.get::<Rotation>(entity).unwrap().0, 0);

    commands.apply(&mut world);
    assert!(world.get::<Initialized>(entity).is_none());
}

#[derive(Component, Debug)]
#[component(on_remove = dense_removed)]
struct Dense;

#[derive(Component, Debug)]
#[component(storage = "sparse", on_remove = sparse_removed)]
struct Sparse;

#[derive(Component, Debug)]
struct Observer;

#[derive(Component)]
struct DenseRemoval(bool);

#[derive(Component)]
struct SparseRemoval;

fn dense_removed(world: &World, entity: Entity, commands: &mut Commands) {
    assert!(world.get::<Dense>(entity).is_none());
    world.query::<&Observer>().for_each(|observer, _| {
        commands.insert(observer, DenseRemoval(world.contains(entity)));
    });
}

fn sparse_removed(world: &World, entity: Entity, commands: &mut Commands) {
    assert!(!world.contains(entity));
    assert!(world.get::<Sparse>(entity).is_none());
    assert!(world.get::<Dense>(entity).is_none());
    world.query::<&Observer>().for_each(|observer, _| {
        commands.insert(observer, SparseRemoval);
    });
}

#[test]
fn removal_hooks_see_the_completed_removal_and_despawn() {
    let mut world = World::new();
    let observer = world.spawn_empty();
    world.insert(observer, Observer).unwrap();
    let entity = world.spawn_empty();
    world.insert(entity, Dense).unwrap();
    world.insert(entity, Sparse).unwrap();
    let mut commands = Commands::default();

    assert!(world.remove::<Dense>(entity).is_some());
    commands.apply(&mut world);
    assert!(world.get::<DenseRemoval>(observer).unwrap().0);
    assert!(world.remove::<DenseRemoval>(observer).is_some());
    assert!(world.remove::<Dense>(entity).is_none());
    commands.apply(&mut world);
    assert!(world.get::<DenseRemoval>(observer).is_none());

    world.insert(entity, Dense).unwrap();
    commands.despawn(entity);
    commands.apply(&mut world);
    assert!(!world.get::<DenseRemoval>(observer).unwrap().0);
    assert!(world.get::<SparseRemoval>(observer).is_some());
}

#[derive(Component)]
#[component(on_add = start)]
struct Start;

#[derive(Component)]
#[component(on_add = followup)]
struct Followup;

#[derive(Component)]
struct Complete;

fn start(_: &World, entity: Entity, commands: &mut Commands) {
    commands.insert(entity, Followup);
}

fn followup(world: &World, entity: Entity, commands: &mut Commands) {
    assert!(world.get::<Followup>(entity).is_some());
    commands.insert(entity, Complete);
}

#[test]
fn hook_commands_apply_one_batch_after_the_stage_commands() {
    let mut world = World::new();
    let entity = world.spawn_empty();
    let mut commands = Commands::default();
    commands.insert(entity, Start);
    commands.remove::<Followup>(entity);

    commands.apply(&mut world);
    assert!(world.get::<Followup>(entity).is_some());
    assert!(world.get::<Complete>(entity).is_none());

    commands.apply(&mut world);
    assert!(world.get::<Complete>(entity).is_some());
}
