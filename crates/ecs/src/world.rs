//! The container every entity and component lives in.

use engine_core::reflection::{TypeKey, TypeRegistration};
use engine_core::{Reflect, TypeRegistry};

use crate::archetype::{Archetypes, Edge, Location};
use crate::entity::Entities;
use crate::storage::{Storage, Stores};
use crate::{Component, ComponentMetadata, Entity, StorageKind};

/// Owner of entities and their components.
///
/// An empty world requires no heap allocation. A play session can own its own
/// world separately from the scene's authoring data (ADR 0004).
#[derive(Debug, Default)]
pub struct World {
    entities: Entities,
    types: TypeRegistry,
    archetypes: Archetypes,
    stores: Stores,
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
        let entity = self.entities.alloc();
        self.archetypes.spawn(entity);
        entity
    }

    /// Removes an entity and its components, invalidating every handle to it.
    ///
    /// Returns whether the entity existed. A handle to an entity that was
    /// already despawned is rejected rather than despawning whatever now
    /// occupies its index.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.free(entity) {
            return false;
        }
        let location = self.archetypes.location(entity);
        self.archetypes.detach(location);
        self.stores.despawn(
            entity,
            location,
            self.archetypes.signature(location.archetype),
        );
        true
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

    /// The shared registry of schemas and component metadata for this world.
    #[must_use]
    pub fn type_registry(&self) -> &TypeRegistry {
        &self.types
    }

    /// Registers a reflected schema. If this is also a component, both kinds
    /// of metadata share the registration regardless of which was added first.
    pub fn register_type<T: Reflect>(&mut self) -> &TypeRegistration {
        self.types.register_reflect::<T>()
    }

    /// Registers a component type during initialization. Insertion also
    /// registers a component on first use.
    pub fn register_component<T: Component>(&mut self) -> &TypeRegistration {
        let entry = self.types.register::<T>();
        if entry.metadata::<ComponentMetadata>().is_none() {
            entry.insert_metadata(ComponentMetadata {
                storage: T::STORAGE,
            });
        }
        entry
    }

    /// Inserts a component, returning its previous value if one existed.
    /// Returns the input unchanged as `Err` if `entity` is no longer alive.
    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) -> Result<Option<T>, T> {
        let Some(location) = self.location(entity) else {
            return Err(component);
        };
        let key = self.register_component::<T>().key();
        let store = self.stores.ensure::<T>(key);
        if let Storage::Sparse(set) = store {
            return Ok(set.insert(entity, component));
        }
        if let Some(previous) = store.get_mut(entity, location) {
            return Ok(Some(std::mem::replace(previous, component)));
        }

        let to = self
            .archetypes
            .transition(location.archetype, key, Edge::Add);
        self.move_columns(location, to, None);
        self.stores.ensure::<T>(key).table_mut(to).push(component);
        self.archetypes.move_entity(location, to);
        Ok(None)
    }

    /// Removes and returns a component. Missing components and stale entities
    /// return `None`.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        let location = self.location(entity)?;
        let key = self.types.get::<T>()?.key();
        let previous = self.stores.get_mut::<T>(key)?.take(entity, location)?;
        if T::STORAGE == StorageKind::Table {
            let to = self
                .archetypes
                .transition(location.archetype, key, Edge::Remove);
            self.move_columns(location, to, Some(key));
            self.archetypes.move_entity(location, to);
        }
        Some(previous)
    }

    /// Borrows a component for a single entity.
    #[must_use]
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        let location = self.location(entity)?;
        let key = self.types.get::<T>()?.key();
        self.stores.get::<T>(key)?.get(entity, location)
    }

    /// Mutably borrows a component for a single entity.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let location = self.location(entity)?;
        let key = self.types.get::<T>()?.key();
        self.stores.get_mut::<T>(key)?.get_mut(entity, location)
    }

    fn location(&self, entity: Entity) -> Option<Location> {
        self.entities
            .contains(entity)
            .then(|| self.archetypes.location(entity))
    }

    fn move_columns(&mut self, from: Location, to: usize, removed: Option<TypeKey>) {
        for &key in self.archetypes.signature(from.archetype) {
            if Some(key) != removed {
                self.stores.move_row(key, from, to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component, Reflect, Debug, PartialEq)]
    #[reflect(version = 2)]
    struct Position(i32);

    #[derive(Component, Debug, PartialEq)]
    #[component(storage = "table")]
    struct Velocity(i32);

    #[derive(Component, Debug, PartialEq)]
    #[component(storage = "sparse")]
    struct Selected<T>(T);

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

    #[test]
    fn component_keys_and_schemas_share_a_deterministic_registration() {
        let mut first = World::new();
        first.register_type::<Position>();
        let position = first.register_component::<Position>().key();
        let selection = first.register_component::<Selected<u8>>().key();

        let mut second = World::new();
        assert_eq!(second.register_component::<Position>().key(), position);
        assert_eq!(second.register_component::<Selected<u8>>().key(), selection);
        assert_eq!(second.register_type::<Position>().key(), position);
        let order = |world: &World| {
            world
                .type_registry()
                .iter()
                .map(TypeRegistration::key)
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&first), vec![position, selection]);
        assert_eq!(order(&first), order(&second));

        for world in [&first, &second] {
            let entry = world.type_registry().get::<Position>().unwrap();
            assert_eq!(entry.schema().unwrap().version, 2);
            assert_eq!(
                entry.metadata::<ComponentMetadata>().unwrap().storage,
                StorageKind::Table
            );
            let entry = world.type_registry().get::<Selected<u8>>().unwrap();
            assert_eq!(
                entry.metadata::<ComponentMetadata>().unwrap().storage,
                StorageKind::Sparse
            );
        }
    }

    #[test]
    fn dense_columns_follow_entities_when_first_and_middle_rows_are_removed() {
        let mut world = World::new();
        let entities = [
            world.spawn_empty(),
            world.spawn_empty(),
            world.spawn_empty(),
        ];
        for (index, &entity) in entities.iter().enumerate() {
            let value = index as i32;
            assert_eq!(world.insert(entity, Position(value)), Ok(None));
            assert_eq!(world.insert(entity, Velocity(value + 10)), Ok(None));
        }
        let [first, middle, last] = entities;

        // Removing one column moves the first entity and fills its old row
        // with the last entity from every column of the source archetype.
        assert_eq!(world.remove::<Velocity>(first), Some(Velocity(10)));
        assert_eq!(world.get::<Position>(first), Some(&Position(0)));
        assert_eq!(world.get::<Velocity>(first), None);
        assert_eq!(world.get::<Position>(last), Some(&Position(2)));
        assert_eq!(world.get::<Velocity>(last), Some(&Velocity(12)));

        assert_eq!(world.insert(first, Velocity(20)), Ok(None));
        assert!(world.despawn(middle));
        assert_eq!(world.insert(first, Position(8)), Ok(Some(Position(0))));
        world.get_mut::<Velocity>(first).unwrap().0 += 1;
        assert_eq!(world.get::<Velocity>(first), Some(&Velocity(21)));
        assert_eq!(world.get::<Position>(last), Some(&Position(2)));
        assert_eq!(world.get::<Velocity>(last), Some(&Velocity(12)));
    }

    #[test]
    fn sparse_toggles_preserve_table_rows_and_reuse_does_not_expose_old_values() {
        let mut world = World::new();
        let first = world.spawn_empty();
        let second = world.spawn_empty();
        world.insert(first, Position(5)).unwrap();
        world.insert(second, Position(6)).unwrap();
        let start = world.location(first).unwrap();

        assert_eq!(world.insert(first, Selected(10_u8)), Ok(None));
        assert_eq!(world.insert(second, Selected(20_u8)), Ok(None));
        assert_eq!(world.remove::<Selected<u8>>(first), Some(Selected(10)));
        assert_eq!(
            world.insert(second, Selected(30_u8)),
            Ok(Some(Selected(20)))
        );
        let after = world.location(first).unwrap();
        assert_eq!((start.archetype, start.row), (after.archetype, after.row));

        assert!(world.despawn(second));
        let reused = world.spawn_empty();
        assert_eq!(second.index(), reused.index());
        assert_eq!(world.get::<Selected<u8>>(reused), None);
        assert_eq!(world.insert(second, Selected(40_u8)), Err(Selected(40)));
        assert_eq!(world.remove::<Position>(second), None);
        assert_eq!(world.get::<Position>(first), Some(&Position(5)));
    }

    #[test]
    fn a_panicking_destructor_cannot_leave_rows_attached_to_a_dead_entity() {
        #[derive(Component, Debug)]
        struct OnDrop(bool);

        impl Drop for OnDrop {
            fn drop(&mut self) {
                assert!(!self.0, "component destructor panicked");
            }
        }

        let mut world = World::new();
        let doomed = world.spawn_empty();
        let survivor = world.spawn_empty();
        world.insert(doomed, OnDrop(true)).unwrap();
        world.insert(doomed, Position(5)).unwrap();
        world.insert(survivor, OnDrop(false)).unwrap();
        world.insert(survivor, Position(6)).unwrap();

        let dropped =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.despawn(doomed)));
        assert!(dropped.is_err());
        assert!(!world.contains(doomed));
        assert_eq!(world.get::<Position>(survivor), Some(&Position(6)));
        assert!(!world.get::<OnDrop>(survivor).unwrap().0);
    }
}
