//! Single-threaded ECS execution in registration order.

use crate::{Commands, Tick, World};

/// Identifies a stage within the schedule that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StageId(usize);

type RunSystem<C> = Box<dyn FnMut(&World, &mut Commands, &mut C, Tick)>;

struct System<C> {
    run: RunSystem<C>,
    last_run: Tick,
}

/// Runs explicit stages in insertion order and each stage's systems in
/// registration order. `C` is context supplied by the caller for each run.
pub struct Schedule<C = ()> {
    stages: Vec<Vec<System<C>>>,
    commands: Commands,
}

impl<C> Default for Schedule<C> {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            commands: Commands::default(),
        }
    }
}

impl<C> Schedule<C> {
    /// Appends a stage and returns its schedule-local handle.
    pub fn add_stage(&mut self) -> StageId {
        let id = StageId(self.stages.len());
        self.stages.push(Vec::new());
        id
    }

    /// Appends a system to the stage. Its last argument is the change tick of
    /// its previous run, or [`Tick::ZERO`] on the first run.
    ///
    /// # Panics
    ///
    /// If the supplied stage is absent from this schedule.
    pub fn add_system(
        &mut self,
        stage: StageId,
        system: impl FnMut(&World, &mut Commands, &mut C, Tick) + 'static,
    ) {
        self.stages[stage.0].push(System {
            run: Box::new(system),
            last_run: Tick::ZERO,
        });
    }

    /// Runs every stage and applies its commands at the boundary. Run a
    /// schedule with the same world throughout its lifetime so that the
    /// systems' previous ticks remain meaningful.
    pub fn run(&mut self, world: &mut World, context: &mut C) {
        // A failed system must not leave its unfinished stage queue for a retry.
        let mut commands = std::mem::take(&mut self.commands);
        for stage in &mut self.stages {
            for system in stage {
                let now = world.advance_tick();
                (system.run)(world, &mut commands, context, system.last_run);
                system.last_run = now;
            }
            // Writes applied here must be newer than every system in the stage.
            world.advance_tick();
            commands.apply(world);
        }
        self.commands = commands;
    }
}
