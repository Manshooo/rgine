use std::any::{TypeId, type_name};
use std::slice;
use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use crate::storage::{SparseIndex, Storage, Store, borrow_conflict};
use crate::tick::ComponentTicks;
use crate::{Component, Entity, Tick, World};

pub trait Data {
    type Access<'w>;
    type Batch<'b>;
    type Item<'row>;
    type Changes<'row>;

    fn borrow(world: &World) -> Self::Access<'_>;
    fn batch<'b, 'w: 'b>(access: &'b mut Self::Access<'w>, id: usize) -> Option<Self::Batch<'b>>;
    fn next<'row, 'b: 'row>(
        batch: &'row mut Self::Batch<'b>,
        entity: Entity,
    ) -> Option<(Self::Item<'row>, Self::Changes<'row>)>;
    fn mark_changed(changes: Self::Changes<'_>, now: Tick);
}

trait AccessMode: Data {
    fn descriptor() -> (TypeId, bool, &'static str);
}

pub struct Read<'w, T> {
    pub(super) store: &'w Store<T>,
    pub(super) values: RwLockReadGuard<'w, Storage<T>>,
}

pub struct Write<'w, T> {
    pub(super) store: &'w Store<T>,
    pub(super) values: RwLockWriteGuard<'w, Storage<T>>,
}

pub struct ReadBatch<'b, T>(ReadRows<'b, T>);
pub struct WriteBatch<'b, T>(WriteRows<'b, T>);
pub struct ChangeMark<'row>(&'row ComponentTicks);

enum ReadRows<'b, T> {
    Dense(slice::Iter<'b, T>),
    Sparse(&'b SparseIndex, &'b [T]),
}

enum WriteRows<'b, T> {
    Dense(slice::IterMut<'b, T>, slice::Iter<'b, ComponentTicks>),
    Sparse(&'b SparseIndex, &'b mut [T], &'b [ComponentTicks]),
}

impl<T: Component> AccessMode for &T {
    fn descriptor() -> (TypeId, bool, &'static str) {
        (TypeId::of::<T>(), false, type_name::<T>())
    }
}

impl<T: Component> Data for &T {
    type Access<'w> = Option<Read<'w, T>>;
    type Batch<'b> = ReadBatch<'b, T>;
    type Item<'row> = &'row T;
    type Changes<'row> = ();

    fn borrow(world: &World) -> Self::Access<'_> {
        let store = world.store::<T>()?;
        Some(Read {
            store,
            values: store.read(),
        })
    }

    fn batch<'b, 'w: 'b>(access: &'b mut Self::Access<'w>, id: usize) -> Option<Self::Batch<'b>> {
        let access = access.as_ref()?;
        let rows = match &*access.values {
            Storage::Table(columns) => ReadRows::Dense(columns.get(&id)?.iter()),
            Storage::Sparse(values) => ReadRows::Sparse(access.store.index.as_ref()?, values),
        };
        Some(ReadBatch(rows))
    }

    fn next<'row, 'b: 'row>(
        batch: &'row mut Self::Batch<'b>,
        entity: Entity,
    ) -> Option<(Self::Item<'row>, Self::Changes<'row>)> {
        let value = match &mut batch.0 {
            ReadRows::Dense(rows) => rows.next(),
            ReadRows::Sparse(index, values) => values.get(index.get(entity)?),
        }?;
        Some((value, ()))
    }

    fn mark_changed(_: Self::Changes<'_>, _: Tick) {}
}

impl<T: Component> AccessMode for &mut T {
    fn descriptor() -> (TypeId, bool, &'static str) {
        (TypeId::of::<T>(), true, type_name::<T>())
    }
}

impl<T: Component> Data for &mut T {
    type Access<'w> = Option<Write<'w, T>>;
    type Batch<'b> = WriteBatch<'b, T>;
    type Item<'row> = &'row mut T;
    type Changes<'row> = ChangeMark<'row>;

    fn borrow(world: &World) -> Self::Access<'_> {
        let store = world.store::<T>()?;
        Some(Write {
            store,
            values: store.write(),
        })
    }

    fn batch<'b, 'w: 'b>(access: &'b mut Self::Access<'w>, id: usize) -> Option<Self::Batch<'b>> {
        let access = access.as_mut()?;
        let rows = match (&mut *access.values, &access.store.ticks) {
            (Storage::Table(columns), Storage::Table(ticks)) => WriteRows::Dense(
                columns.get_mut(&id)?.iter_mut(),
                ticks.get(&id).expect("table component ticks").iter(),
            ),
            (Storage::Sparse(values), Storage::Sparse(ticks)) => {
                WriteRows::Sparse(access.store.index.as_ref()?, values, ticks)
            }
            _ => unreachable!("component ticks use the same storage kind as their values"),
        };
        Some(WriteBatch(rows))
    }

    fn next<'row, 'b: 'row>(
        batch: &'row mut Self::Batch<'b>,
        entity: Entity,
    ) -> Option<(Self::Item<'row>, Self::Changes<'row>)> {
        let (value, ticks) = match &mut batch.0 {
            WriteRows::Dense(rows, ticks) => (rows.next()?, ticks.next().expect("component ticks")),
            WriteRows::Sparse(index, values, ticks) => {
                let row = index.get(entity)?;
                (&mut values[row], &ticks[row])
            }
        };
        Some((value, ChangeMark(ticks)))
    }

    fn mark_changed(changes: Self::Changes<'_>, now: Tick) {
        changes.0.mark_changed(now);
    }
}

macro_rules! tuple_data {
    ($(($mode:ident, $row:ident, $index:tt)),+ $(,)?) => {
        impl<$($mode: AccessMode),+> Data for ($($mode,)+) {
            type Access<'w> = ($($mode::Access<'w>,)+);
            type Batch<'b> = ($($mode::Batch<'b>,)+);
            type Item<'row> = ($($mode::Item<'row>,)+);
            type Changes<'row> = ($($mode::Changes<'row>,)+);

            fn borrow(world: &World) -> Self::Access<'_> {
                let types = [$($mode::descriptor(),)+];
                for (index, &(id, write, name)) in types.iter().enumerate() {
                    if types[..index].iter().any(|&(other, other_write, _)| id == other && (write || other_write)) {
                        borrow_conflict(name);
                    }
                }
                ($($mode::borrow(world),)+)
            }

            fn batch<'b, 'w: 'b>(access: &'b mut Self::Access<'w>, id: usize) -> Option<Self::Batch<'b>> {
                Some(($($mode::batch(&mut access.$index, id)?,)+))
            }

            fn next<'row, 'b: 'row>(
                batch: &'row mut Self::Batch<'b>,
                entity: Entity,
            ) -> Option<(Self::Item<'row>, Self::Changes<'row>)> {
                // All dense columns advance even when one sparse value is missing.
                $(let $row = $mode::next(&mut batch.$index, entity);)+
                $(let $row = $row?;)+
                Some((($($row.0,)+), ($($row.1,)+)))
            }

            fn mark_changed(changes: Self::Changes<'_>, now: Tick) {
                $($mode::mark_changed(changes.$index, now);)+
            }
        }
    };
}

tuple_data!((A, a, 0), (B, b, 1));
tuple_data!((A, a, 0), (B, b, 1), (C, c, 2));
tuple_data!((A, a, 0), (B, b, 1), (C, c, 2), (D, d, 3));
