use std::marker::PhantomData;

use engine_core::reflection::TypeKey;

use crate::storage::SparseIndex;
use crate::{Component, Entity, StorageKind, World};

/// A type-level filter accepted by [`World::query_filtered`].
/// Use [`With`], [`Without`], or a tuple of up to four filters combined with AND.
pub trait QueryFilter: Filter {}
impl<F: Filter> QueryFilter for F {}

/// Matches entities with `T`, without borrowing its values.
pub struct With<T>(PhantomData<fn() -> T>);

/// Matches entities without `T`, without borrowing its values.
pub struct Without<T>(PhantomData<fn() -> T>);

pub trait Filter {
    type State<'w>: Prepared;
    fn prepare(world: &World) -> Self::State<'_>;
}

pub trait Prepared {
    fn matches_archetype(&self, signature: &[TypeKey]) -> bool;
    fn matches_entity(&self, entity: Entity) -> bool;
}

enum Membership<'w> {
    Missing,
    Table(TypeKey),
    Sparse(&'w SparseIndex),
}

pub struct Presence<'w, const PRESENT: bool>(Membership<'w>);

impl<const PRESENT: bool> Presence<'_, PRESENT> {
    fn for_type<T: Component>(world: &World) -> Presence<'_, PRESENT> {
        let membership = match world.type_registry().get::<T>() {
            Some(entry) if T::STORAGE == StorageKind::Table => Membership::Table(entry.key()),
            Some(entry) => world
                .sparse_index(entry.key())
                .map_or(Membership::Missing, Membership::Sparse),
            None => Membership::Missing,
        };
        Presence(membership)
    }
}

impl<const PRESENT: bool> Prepared for Presence<'_, PRESENT> {
    fn matches_archetype(&self, signature: &[TypeKey]) -> bool {
        match self.0 {
            Membership::Missing => !PRESENT,
            Membership::Table(key) => signature.binary_search(&key).is_ok() == PRESENT,
            Membership::Sparse(_) => true,
        }
    }

    fn matches_entity(&self, entity: Entity) -> bool {
        match self.0 {
            Membership::Sparse(index) => index.get(entity).is_some() == PRESENT,
            _ => true,
        }
    }
}

impl<T: Component> Filter for With<T> {
    type State<'w> = Presence<'w, true>;

    fn prepare(world: &World) -> Self::State<'_> {
        Presence::for_type::<T>(world)
    }
}

impl<T: Component> Filter for Without<T> {
    type State<'w> = Presence<'w, false>;

    fn prepare(world: &World) -> Self::State<'_> {
        Presence::for_type::<T>(world)
    }
}

impl Filter for () {
    type State<'w> = ();

    fn prepare(_: &World) -> Self::State<'_> {}
}

impl Prepared for () {
    fn matches_archetype(&self, _: &[TypeKey]) -> bool {
        true
    }
    fn matches_entity(&self, _: Entity) -> bool {
        true
    }
}

macro_rules! tuple_filter {
    ($(($filter:ident, $index:tt)),+ $(,)?) => {
        impl<$($filter: Filter),+> Filter for ($($filter,)+) {
            type State<'w> = ($($filter::State<'w>,)+);

            fn prepare(world: &World) -> Self::State<'_> {
                ($($filter::prepare(world),)+)
            }
        }

        impl<$($filter: Prepared),+> Prepared for ($($filter,)+) {
            fn matches_archetype(&self, signature: &[TypeKey]) -> bool {
                $(self.$index.matches_archetype(signature))&&+
            }

            fn matches_entity(&self, entity: Entity) -> bool {
                $(self.$index.matches_entity(entity))&&+
            }
        }
    };
}

tuple_filter!((A, 0), (B, 1));
tuple_filter!((A, 0), (B, 1), (C, 2));
tuple_filter!((A, 0), (B, 1), (C, 2), (D, 3));
