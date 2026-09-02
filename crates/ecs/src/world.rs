//! The container every entity and component lives in.

use crate::Entity;
use crate::entity::Entities;

/// Owner of all entities and, from the next pull request on, of their
/// components.
///
/// A world is a plain value: creating one costs an allocation-free `Vec`, and
/// several can coexist. The editor relies on that - a play session runs in a
/// world of its own, separate from the one the scene is authored in (ADR 0004).
#[derive(Debug, Default)]
pub struct World {
    entities: Entities,
}

impl World {
    /// Creates an empty world.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an entity with no components and returns its handle.
    ///
    /// The handle is valid until [`World::despawn`] is called with it.
    pub fn spawn_empty(&mut self) -> Entity {
        self.entities.alloc()
    }

    /// Removes an entity, invalidating every handle to it.
    ///
    /// Returns whether the entity existed. A handle to an entity that was
    /// already despawned is rejected rather than despawning whatever now
    /// occupies its index.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        self.entities.free(entity)
    }

    /// Whether the handle names an entity that is alive.
    #[must_use]
    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.contains(entity)
    }

    /// How many entities are alive.
    #[must_use]
    pub fn entity_count(&self) -> u32 {
        self.entities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_entity_is_contained_until_despawned() {
        let mut world = World::new();

        let entity = world.spawn_empty();
        assert!(world.contains(entity));
        assert_eq!(world.entity_count(), 1);

        assert!(world.despawn(entity));
        assert!(!world.contains(entity));
        assert_eq!(world.entity_count(), 0);
    }

    #[test]
    fn despawning_twice_reports_the_second_attempt_as_a_miss() {
        let mut world = World::new();
        let entity = world.spawn_empty();

        assert!(world.despawn(entity));
        assert!(!world.despawn(entity));
    }

    // The failure this guards against is a stale handle despawning the entity
    // that took over its index, which is silent data loss rather than a crash.
    #[test]
    fn a_stale_handle_does_not_despawn_the_entity_that_reused_its_index() {
        let mut world = World::new();
        let stale = world.spawn_empty();
        world.despawn(stale);

        let current = world.spawn_empty();
        assert_eq!(stale.index(), current.index());

        assert!(!world.despawn(stale));
        assert!(world.contains(current));
    }
}
