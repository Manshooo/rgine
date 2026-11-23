//! Whole-component borrows, archetype iteration and entity access.

mod fetch;
mod filter;

use std::sync::RwLockReadGuard;
use std::{fmt, ops::Deref};

use crate::archetype::Location;
use crate::storage::{Row, Storage, Store};
use crate::{Component, Entity, World};
use filter::Prepared;

pub use filter::{QueryFilter, With, Without};

/// The component references requested by a query.
///
/// Use a shared `&T`, an exclusive `&mut T`, or a tuple of up to four
/// component references.
pub trait QueryData: fetch::Data {}
impl<D: fetch::Data> QueryData for D {}

/// A shared reference to one component that keeps its type borrowed.
///
/// Dereference the guard to read the value. Use `Option::as_deref` on the
/// result of [`World::get`] when you need a short-lived `Option<&T>`.
pub struct ComponentRef<'w, T> {
    values: RwLockReadGuard<'w, Storage<T>>,
    row: Row,
}

impl<'w, T: Component> ComponentRef<'w, T> {
    pub(crate) fn new(store: &'w Store<T>, row: Row) -> Option<Self> {
        let values = store.read();
        values.get(row)?;
        Some(Self { values, row })
    }
}

impl<T> Deref for ComponentRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.values.get(self.row).expect("borrowed component row")
    }
}

impl<T: fmt::Debug> fmt::Debug for ComponentRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

/// A query that holds its component borrows until it is dropped.
///
/// ```
/// use engine_ecs::{Component, With, World};
///
/// #[derive(Component, Debug)]
/// struct Position(i32);
/// #[derive(Component, Debug)]
/// struct Velocity(i32);
/// #[derive(Component, Debug)]
/// #[component(storage = "sparse")]
/// struct Active;
///
/// let mut world = World::new();
/// let player = world.spawn_empty();
/// world.insert(player, Position(10)).unwrap();
/// world.insert(player, Velocity(2)).unwrap();
/// world.insert(player, Active).unwrap();
/// world.query_filtered::<(&mut Position, &Velocity), With<Active>>()
///     .for_each(|_, (position, velocity)| position.0 += velocity.0);
/// assert_eq!(world.get::<Position>(player).unwrap().0, 12);
/// ```
pub struct Query<'w, D: QueryData, F: QueryFilter = ()> {
    world: &'w World,
    access: D::Access<'w>,
    filter: F::State<'w>,
}

impl<'w, D: QueryData, F: QueryFilter> Query<'w, D, F> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self {
            world,
            access: D::borrow(world),
            filter: F::prepare(world),
        }
    }

    /// Visits matching entities in archetype creation order and then in each
    /// archetype's current row order. Iteration allocates no memory.
    ///
    /// Read and mutable references are scoped to each invocation of `visit`.
    pub fn for_each(&mut self, mut visit: impl for<'row> FnMut(Entity, D::Item<'row>)) {
        for (id, (signature, entities)) in self.world.query_archetypes().enumerate() {
            if !self.filter.matches_archetype(signature) || entities.is_empty() {
                continue;
            }
            let Some(mut batch) = D::batch(&mut self.access, id) else {
                continue;
            };
            for &entity in entities {
                let item = D::next(&mut batch, entity);
                if let Some(item) = item.filter(|_| self.filter.matches_entity(entity)) {
                    visit(entity, item);
                }
            }
        }
    }

    fn location(&self, entity: Entity) -> Option<Location> {
        let location = self.world.location(entity)?;
        (self
            .filter
            .matches_archetype(self.world.signature(location.archetype))
            && self.filter.matches_entity(entity))
        .then_some(location)
    }
}

impl<T: Component, F: QueryFilter> Query<'_, &T, F> {
    /// Gets the component of a matching, live entity using this query's borrow.
    #[must_use]
    pub fn get(&self, entity: Entity) -> Option<&T> {
        let access = self.access.as_ref()?;
        access
            .values
            .get(access.store.row(entity, self.location(entity)?)?)
    }
}

impl<T: Component, F: QueryFilter> Query<'_, &mut T, F> {
    /// Reads the component of a matching, live entity using this query's borrow.
    #[must_use]
    pub fn get(&self, entity: Entity) -> Option<&T> {
        let access = self.access.as_ref()?;
        access
            .values
            .get(access.store.row(entity, self.location(entity)?)?)
    }

    /// Mutably borrows the component of one matching, live entity.
    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        let location = self.location(entity)?;
        let access = self.access.as_mut()?;
        access.values.get_mut(access.store.row(entity, location)?)
    }

    /// Mutably borrows components in the order of the supplied entities.
    ///
    /// Returns `None` for repeated entities, stale handles, missing components
    /// or entities excluded by the filter. The empty input always succeeds.
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        entities: [Entity; N],
    ) -> Option<[&mut T; N]> {
        if N == 0 {
            return Some(std::array::from_fn(|_| unreachable!("empty request")));
        }
        let store = self.access.as_ref()?.store;
        let mut rows = [Row::Sparse(0); N];
        for (row, entity) in rows.iter_mut().zip(entities) {
            *row = store.row(entity, self.location(entity)?)?;
        }
        self.access.as_mut()?.values.get_disjoint_mut(rows)
    }
}
