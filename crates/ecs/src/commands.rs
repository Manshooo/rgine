//! Deferred structural changes for an ECS stage.

use crate::{Component, Entity, World};

type ComponentEdit = Box<dyn FnOnce(&mut World, Entity) + Send>;

enum Target {
    Existing(Entity),
    LastSpawn,
}

enum Command {
    Spawn,
    Despawn(Entity),
    Edit(Target, ComponentEdit),
}

/// A queue of structural changes recorded by systems during a stage.
///
/// Give systems a shared reference to the world and a mutable reference to
/// this queue. The queue leaves entity and component membership unchanged
/// until the stage runner takes an exclusive world borrow and calls
/// [`apply`](Self::apply).
#[derive(Default)]
pub struct Commands {
    queue: Vec<Command>,
}

impl Commands {
    /// Queues a spawn and returns a builder that can attach initial components.
    ///
    /// The world allocates the entity handle when the spawn is applied.
    pub fn spawn_empty(&mut self) -> SpawnCommands<'_> {
        self.queue.push(Command::Spawn);
        SpawnCommands { commands: self }
    }

    /// Queues the removal of an entity and all its components.
    pub fn despawn(&mut self, entity: Entity) {
        self.queue.push(Command::Despawn(entity));
    }

    /// Queues a component insertion, replacing any existing value of its type.
    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        self.queue_insert(Target::Existing(entity), component);
    }

    /// Queues removal of a component if it exists.
    pub fn remove<T: Component>(&mut self, entity: Entity) {
        self.queue.push(Command::Edit(
            Target::Existing(entity),
            Box::new(|world, entity| drop(world.remove::<T>(entity))),
        ));
    }

    /// Drains the queue in recording order at the stage boundary.
    ///
    /// Commands targeting a stale entity are ignored, even if a spawn has
    /// already reused its index. Apply the queue to the world that supplied
    /// its existing entity handles. Changes are recorded at that world's
    /// current tick.
    pub fn apply(&mut self, world: &mut World) {
        let mut last_spawn = None;
        for command in self.queue.drain(..) {
            match command {
                Command::Spawn => last_spawn = Some(world.spawn_empty()),
                Command::Despawn(entity) => {
                    world.despawn(entity);
                }
                Command::Edit(target, edit) => {
                    let entity = match target {
                        Target::Existing(entity) => entity,
                        // The builder borrows the queue, so its edits always
                        // follow their own spawn before another can be queued.
                        Target::LastSpawn => last_spawn.expect("spawn queued before its edits"),
                    };
                    edit(world, entity);
                }
            }
        }
    }

    fn queue_insert<T: Component>(&mut self, target: Target, component: T) {
        self.queue.push(Command::Edit(
            target,
            Box::new(move |world, entity| drop(world.insert(entity, component))),
        ));
    }
}

/// Records initial components for a spawn queued by [`Commands::spawn_empty`].
pub struct SpawnCommands<'a> {
    commands: &'a mut Commands,
}

impl SpawnCommands<'_> {
    /// Queues an insertion on the new entity and returns the builder for chaining.
    pub fn insert<T: Component>(&mut self, component: T) -> &mut Self {
        self.commands.queue_insert(Target::LastSpawn, component);
        self
    }
}
