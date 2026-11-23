//! The container every entity and component lives in.

use std::any::TypeId;
use std::collections::HashSet;

use engine_core::reflection::{TypeKey, TypeRegistration};
use engine_core::{Reflect, TypeRegistry};

use crate::archetype::{Archetypes, Edge, Location};
use crate::entity::Entities;
use crate::relation::Relations;
use crate::storage::{SparseIndex, Store, Stores};
use crate::{
    Commands, Component, ComponentHook, ComponentLifecycle, ComponentMetadata, ComponentRef,
    Entity, Query, QueryData, QueryFilter, Relation, RelationError, RequiredComponent, StorageKind,
    Tick,
};

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
    relations: Relations,
    tick: Tick,
    hook_commands: Commands,
}

impl World {
    /// Creates an empty world.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current simulation tick.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Advances the simulation clock and returns the new tick.
    pub fn advance_tick(&mut self) -> Tick {
        self.tick = self.tick.next();
        if self.tick.needs_aging() {
            // Bound the age of stored timestamps before the clock wraps.
            self.stores.age_ticks(self.tick);
        }
        self.tick
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
    ///
    /// Removal hooks run after the entity and all of its components are gone.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.free(entity) {
            return false;
        }
        self.relations.despawn(entity);
        let location = self.archetypes.location(entity);
        self.archetypes.detach(location);
        let mut hooks = Vec::new();
        let types = &self.types;
        let removed = self.stores.despawn(
            entity,
            location,
            self.archetypes.signature(location.archetype),
            |key| {
                let hook = types
                    .get_by_key(key)
                    .and_then(TypeRegistration::metadata::<ComponentLifecycle>)
                    .and_then(|lifecycle| lifecycle.on_remove);
                hooks.extend(hook);
            },
        );
        self.run_hooks(entity, hooks);
        drop(removed);
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

    /// Adds an edge of type `R`. Returns whether the edge was new.
    ///
    /// An invalid endpoint or cycle leaves the relationship unchanged.
    pub fn add_relation<R: Relation>(
        &mut self,
        source: Entity,
        target: Entity,
    ) -> Result<bool, RelationError> {
        for entity in [source, target] {
            if !self.contains(entity) {
                return Err(RelationError::StaleEntity(entity));
            }
        }
        self.relations.insert::<R>(source, target)
    }

    /// Removes an edge of type `R` if it exists.
    pub fn remove_relation<R: Relation>(&mut self, source: Entity, target: Entity) -> bool {
        self.relations.remove::<R>(source, target)
    }

    /// Visits the direct targets of an entity in entity handle order.
    pub fn relation_targets<R: Relation>(
        &self,
        source: Entity,
    ) -> impl Iterator<Item = Entity> + '_ {
        self.relations.targets::<R>(source)
    }

    /// Visits the direct sources pointing at an entity in entity handle order.
    pub fn relation_sources<R: Relation>(
        &self,
        target: Entity,
    ) -> impl Iterator<Item = Entity> + '_ {
        self.relations.sources::<R>(target)
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
            if !T::REQUIRED.is_empty() || T::ON_ADD.is_some() || T::ON_REMOVE.is_some() {
                entry.insert_metadata(ComponentLifecycle {
                    required: T::REQUIRED,
                    on_add: T::ON_ADD,
                    on_remove: T::ON_REMOVE,
                });
            }
            self.stores.ensure::<T>(entry.key());
        }
        entry
    }

    /// Inserts a component, returning its previous value if one existed.
    /// Returns the input unchanged as `Err` if `entity` is no longer alive.
    ///
    /// Fills missing requirements transitively while preserving existing
    /// values. Add hooks run after all requirements are present. Replacing
    /// a component does not call its add hook again.
    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) -> Result<Option<T>, T> {
        let Some(location) = self.location(entity) else {
            return Err(component);
        };
        let previous = self.insert_value(entity, location, component);
        if !T::REQUIRED.is_empty() || (previous.is_none() && T::ON_ADD.is_some()) {
            let mut hooks = Vec::new();
            if !T::REQUIRED.is_empty() {
                // The set is used only for membership. The stack walks
                // requirements depth-first in declaration order.
                let mut visited = HashSet::from([TypeId::of::<T>()]);
                let mut pending: Vec<_> = T::REQUIRED.iter().rev().collect();
                while let Some(required) = pending.pop() {
                    if visited.insert(required.type_id()) {
                        let children = (required.install)(self, entity, &mut hooks);
                        pending.extend(children.iter().rev());
                    }
                }
            }
            if previous.is_none() {
                hooks.extend(T::ON_ADD);
            }
            self.run_hooks(entity, hooks);
        }
        Ok(previous)
    }

    fn insert_value<T: Component>(
        &mut self,
        entity: Entity,
        location: Location,
        component: T,
    ) -> Option<T> {
        let key = self.register_component::<T>().key();
        let store = self.stores.ensure::<T>(key);
        if T::STORAGE == StorageKind::Sparse {
            return store.insert_sparse(entity, component, self.tick);
        }
        if let Some(previous) = store.get_mut(entity, location, self.tick) {
            return Some(std::mem::replace(previous, component));
        }

        let to = self
            .archetypes
            .transition(location.archetype, key, Edge::Add);
        self.move_columns(location, to, None);
        self.stores
            .ensure::<T>(key)
            .push_table(to, component, self.tick);
        self.archetypes.move_entity(location, to);
        None
    }

    pub(crate) fn insert_required<T: Component + Default>(
        &mut self,
        entity: Entity,
        hooks: &mut Vec<ComponentHook>,
    ) -> &'static [RequiredComponent] {
        if self.get::<T>(entity).is_none() {
            let location = self.archetypes.location(entity);
            self.insert_value(entity, location, T::default());
            hooks.extend(T::ON_ADD);
        }
        T::REQUIRED
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
        self.run_hooks(entity, T::ON_REMOVE);
        Some(previous)
    }

    /// Borrows a component for a single entity. The shared borrow stays alive
    /// until the returned guard is dropped.
    ///
    /// # Panics
    ///
    /// If the component type is already mutably borrowed by a query.
    #[must_use]
    pub fn get<T: Component>(&self, entity: Entity) -> Option<ComponentRef<'_, T>> {
        let location = self.location(entity)?;
        let store = self.store::<T>()?;
        ComponentRef::new(store, store.row(entity, location)?)
    }

    /// Mutably borrows a component and marks it changed at the current tick.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let location = self.location(entity)?;
        let key = self.types.get::<T>()?.key();
        self.stores
            .get_mut::<T>(key)?
            .get_mut(entity, location, self.tick)
    }

    /// Borrows the component types in `D` for iteration and entity access.
    ///
    /// # Panics
    ///
    /// If `D` or a live query has an incompatible borrow of the same type.
    pub fn query<D: QueryData>(&self) -> Query<'_, D> {
        self.query_filtered::<D, ()>()
    }

    /// Borrows component types like [`World::query`] and limits matching
    /// entities with filters. Change filters match the current tick.
    ///
    /// # Panics
    ///
    /// If `D` or a live query has an incompatible borrow of the same type.
    pub fn query_filtered<D: QueryData, F: QueryFilter>(&self) -> Query<'_, D, F> {
        self.query_filtered_since::<D, F>(self.tick.previous())
    }

    /// Borrows components with filters that observe changes after `last_run`
    /// through the current tick, inclusive. Start with [`Tick::ZERO`] and
    /// retain a tick from this same world for subsequent system runs.
    ///
    /// # Panics
    ///
    /// If `D` or a live query has an incompatible borrow of the same type.
    pub fn query_filtered_since<D: QueryData, F: QueryFilter>(
        &self,
        last_run: Tick,
    ) -> Query<'_, D, F> {
        Query::new(self, last_run)
    }

    pub(crate) fn query_archetypes(&self) -> impl Iterator<Item = (&[TypeKey], &[Entity])> {
        self.archetypes.iter()
    }

    pub(crate) fn signature(&self, archetype: usize) -> &[TypeKey] {
        self.archetypes.signature(archetype)
    }

    pub(crate) fn store<T: Component>(&self) -> Option<&Store<T>> {
        self.stores.get::<T>(self.types.get::<T>()?.key())
    }

    pub(crate) fn sparse_index(&self, key: TypeKey) -> Option<&SparseIndex> {
        self.stores.sparse_index(key)
    }

    pub(crate) fn location(&self, entity: Entity) -> Option<Location> {
        self.entities
            .contains(entity)
            .then(|| self.archetypes.location(entity))
    }

    pub(crate) fn take_hook_commands(&mut self) -> Commands {
        std::mem::take(&mut self.hook_commands)
    }

    fn run_hooks(&mut self, entity: Entity, hooks: impl IntoIterator<Item = ComponentHook>) {
        let mut commands = Commands::default();
        for hook in hooks {
            hook(self, entity, &mut commands);
        }
        self.hook_commands.append(&mut commands);
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
    use crate::{Added, Changed};

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
    fn change_filters_cross_world_tick_wraparound() {
        let mut world = World {
            tick: Tick::INITIAL.previous().previous().previous(),
            ..World::default()
        };
        let old = world.spawn_empty();
        world.insert(old, Position(1)).unwrap();
        let last_run = world.tick();

        assert_eq!(world.advance_tick(), Tick::INITIAL.previous().previous());
        let new = world.spawn_empty();
        world.insert(new, Position(2)).unwrap();
        assert_eq!(world.advance_tick(), Tick::ZERO);
        world.get_mut::<Position>(old).unwrap().0 = 3;

        let changes = world.query_filtered_since::<&Position, Changed<Position>>(last_run);
        assert_eq!(changes.get(old), Some(&Position(3)));
        assert_eq!(changes.get(new), Some(&Position(2)));
        let additions = world.query_filtered_since::<&Position, Added<Position>>(last_run);
        assert!(additions.get(old).is_none());
        assert_eq!(additions.get(new), Some(&Position(2)));
        let current = world.query_filtered::<&Position, Changed<Position>>();
        assert!(current.get(old).is_some());
        assert!(current.get(new).is_none());
    }

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
        assert_eq!(world.get::<Position>(first).as_deref(), Some(&Position(0)));
        assert_eq!(world.get::<Velocity>(first).as_deref(), None);
        assert_eq!(world.get::<Position>(last).as_deref(), Some(&Position(2)));
        assert_eq!(world.get::<Velocity>(last).as_deref(), Some(&Velocity(12)));

        assert_eq!(world.insert(first, Velocity(20)), Ok(None));
        assert!(world.despawn(middle));
        assert_eq!(world.insert(first, Position(8)), Ok(Some(Position(0))));
        world.get_mut::<Velocity>(first).unwrap().0 += 1;
        assert_eq!(world.get::<Velocity>(first).as_deref(), Some(&Velocity(21)));
        assert_eq!(world.get::<Position>(last).as_deref(), Some(&Position(2)));
        assert_eq!(world.get::<Velocity>(last).as_deref(), Some(&Velocity(12)));
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
        assert_eq!(world.get::<Selected<u8>>(reused).as_deref(), None);
        assert_eq!(world.insert(second, Selected(40_u8)), Err(Selected(40)));
        assert_eq!(world.remove::<Position>(second), None);
        assert_eq!(world.get::<Position>(first).as_deref(), Some(&Position(5)));
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
        assert_eq!(
            world.get::<Position>(survivor).as_deref(),
            Some(&Position(6))
        );
        assert!(!world.get::<OnDrop>(survivor).unwrap().0);
    }
}
