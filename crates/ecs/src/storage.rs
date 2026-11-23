//! Type-owned value storage. Each table column is a contiguous `Vec<T>`.

use std::any::{Any, type_name};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

use engine_core::reflection::TypeKey;

use crate::archetype::Location;
use crate::{Component, Entity, StorageKind};

#[derive(Default)]
pub(crate) struct Stores(BTreeMap<TypeKey, Box<dyn ErasedStorage>>);

impl fmt::Debug for Stores {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stores")
            .field("types", &self.0.len())
            .finish()
    }
}

impl Stores {
    pub(crate) fn get<T: Component>(&self, key: TypeKey) -> Option<&Store<T>> {
        self.0.get(&key)?.as_any().downcast_ref()
    }

    pub(crate) fn get_mut<T: Component>(&mut self, key: TypeKey) -> Option<&mut Store<T>> {
        self.0.get_mut(&key)?.as_any_mut().downcast_mut()
    }

    pub(crate) fn ensure<T: Component>(&mut self, key: TypeKey) -> &mut Store<T> {
        self.0
            .entry(key)
            .or_insert_with(|| Box::new(Store::<T>::new()))
            .as_any_mut()
            .downcast_mut()
            .expect("component key has one Rust type")
    }

    pub(crate) fn sparse_index(&self, key: TypeKey) -> Option<&SparseIndex> {
        self.0.get(&key)?.sparse_index()
    }

    pub(crate) fn move_row(&mut self, key: TypeKey, from: Location, to: usize) {
        self.0
            .get_mut(&key)
            .expect("table component store")
            .move_row(from, to);
    }

    pub(crate) fn despawn(&mut self, entity: Entity, location: Location, signature: &[TypeKey]) {
        // Leave every store consistent before running a component's destructor.
        let mut removed = Vec::with_capacity(signature.len());
        for key in signature {
            removed.push(
                self.0
                    .get_mut(key)
                    .expect("table component store")
                    .take_row(location),
            );
        }
        removed.extend(
            self.0
                .values_mut()
                .filter_map(|store| store.take_sparse(entity)),
        );
    }
}

trait ErasedStorage: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn sparse_index(&self) -> Option<&SparseIndex>;
    fn move_row(&mut self, from: Location, to: usize);
    fn take_row(&mut self, location: Location) -> Box<dyn Any>;
    fn take_sparse(&mut self, entity: Entity) -> Option<Box<dyn Any>>;
}

/// The sparse index only changes during structural edits with an exclusive
/// world borrow, so filters can read it independently of value borrows.
pub(crate) struct Store<T> {
    pub(crate) index: Option<SparseIndex>,
    values: RwLock<Storage<T>>,
}

impl<T: Component> Store<T> {
    fn new() -> Self {
        match T::STORAGE {
            StorageKind::Table => Self {
                index: None,
                values: RwLock::new(Storage::Table(BTreeMap::new())),
            },
            StorageKind::Sparse => Self {
                index: Some(SparseIndex::default()),
                values: RwLock::new(Storage::Sparse(Vec::new())),
            },
        }
    }

    pub(crate) fn read(&self) -> RwLockReadGuard<'_, Storage<T>> {
        borrow_values::<T, _>(self.values.try_read())
    }

    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, Storage<T>> {
        borrow_values::<T, _>(self.values.try_write())
    }

    pub(crate) fn row(&self, entity: Entity, location: Location) -> Option<Row> {
        match &self.index {
            Some(index) => Some(Row::Sparse(index.get(entity)?)),
            None => Some(Row::Table(location)),
        }
    }

    fn values_mut(&mut self) -> &mut Storage<T> {
        self.values
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn get_mut(&mut self, entity: Entity, location: Location) -> Option<&mut T> {
        let row = self.row(entity, location)?;
        self.values_mut().get_mut(row)
    }

    pub(crate) fn table_mut(&mut self, archetype: usize) -> &mut Vec<T> {
        let Storage::Table(columns) = self.values_mut() else {
            unreachable!("table component storage")
        };
        columns.entry(archetype).or_default()
    }

    pub(crate) fn insert_sparse(&mut self, entity: Entity, value: T) -> Option<T> {
        let index = self.index.as_mut().expect("sparse component index");
        let Storage::Sparse(values) = self
            .values
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        else {
            unreachable!("sparse component storage")
        };
        if let Some(row) = index.get(entity) {
            return Some(std::mem::replace(&mut values[row], value));
        }
        index.rows.insert(entity, values.len());
        index.entities.push(entity);
        values.push(value);
        None
    }

    pub(crate) fn take(&mut self, entity: Entity, location: Location) -> Option<T> {
        if let Some(index) = &mut self.index {
            let row = index.remove(entity)?;
            let Storage::Sparse(values) = self.values_mut() else {
                unreachable!("sparse component storage")
            };
            return Some(values.swap_remove(row));
        }
        let Storage::Table(columns) = self.values_mut() else {
            unreachable!("table component storage")
        };
        Some(
            columns
                .get_mut(&location.archetype)?
                .swap_remove(location.row),
        )
    }
}

impl<T: Component> ErasedStorage for Store<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn sparse_index(&self) -> Option<&SparseIndex> {
        self.index.as_ref()
    }

    fn move_row(&mut self, from: Location, to: usize) {
        let value = self.table_mut(from.archetype).swap_remove(from.row);
        self.table_mut(to).push(value);
    }

    fn take_row(&mut self, location: Location) -> Box<dyn Any> {
        Box::new(self.table_mut(location.archetype).swap_remove(location.row))
    }

    fn take_sparse(&mut self, entity: Entity) -> Option<Box<dyn Any>> {
        self.index.as_ref()?;
        self.take(entity, Location::default())
            .map(|value| Box::new(value) as Box<dyn Any>)
    }
}

fn borrow_values<T: Component, G>(borrow: Result<G, TryLockError<G>>) -> G {
    match borrow {
        Ok(guard) => guard,
        // A panic inside a query can affect values but cannot change the
        // structure of the store while the shared world borrow is alive.
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => borrow_conflict(type_name::<T>()),
    }
}

pub(crate) fn borrow_conflict(name: &str) -> ! {
    panic!("conflicting borrow of component `{name}`")
}

#[derive(Clone, Copy)]
pub(crate) enum Row {
    Table(Location),
    Sparse(usize),
}

impl Row {
    fn coordinates(self) -> (usize, usize) {
        match self {
            Self::Table(location) => (location.archetype, location.row),
            Self::Sparse(row) => (0, row),
        }
    }
}

pub(crate) enum Storage<T> {
    Table(BTreeMap<usize, Vec<T>>),
    Sparse(Vec<T>),
}

impl<T> Storage<T> {
    pub(crate) fn get(&self, row: Row) -> Option<&T> {
        match (self, row) {
            (Self::Table(columns), Row::Table(location)) => {
                columns.get(&location.archetype)?.get(location.row)
            }
            (Self::Sparse(values), Row::Sparse(index)) => values.get(index),
            _ => unreachable!("row belongs to component storage"),
        }
    }

    pub(crate) fn get_mut(&mut self, row: Row) -> Option<&mut T> {
        match (self, row) {
            (Self::Table(columns), Row::Table(location)) => {
                columns.get_mut(&location.archetype)?.get_mut(location.row)
            }
            (Self::Sparse(values), Row::Sparse(index)) => values.get_mut(index),
            _ => unreachable!("row belongs to component storage"),
        }
    }

    pub(crate) fn get_disjoint_mut<const N: usize>(
        &mut self,
        rows: [Row; N],
    ) -> Option<[&mut T; N]> {
        let mut requested =
            std::array::from_fn::<_, N, _>(|index| (rows[index].coordinates(), index));
        requested.sort_unstable();
        if requested.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return None;
        }
        match self {
            Self::Table(columns) => {
                if let (Some(first), Some(last)) = (requested.first(), requested.last()) {
                    let columns = columns
                        .range_mut(first.0.0..=last.0.0)
                        .map(|(&id, rows)| (id, rows.as_mut_slice()));
                    disjoint_rows(columns, requested)
                } else {
                    disjoint_rows(std::iter::empty(), requested)
                }
            }
            Self::Sparse(values) => {
                disjoint_rows(std::iter::once((0, values.as_mut_slice())), requested)
            }
        }
    }
}

fn disjoint_rows<'a, T, const N: usize>(
    columns: impl IntoIterator<Item = (usize, &'a mut [T])>,
    requested: [((usize, usize), usize); N],
) -> Option<[&'a mut T; N]> {
    let mut requested = requested.into_iter().peekable();
    let mut result = [const { None }; N];
    for (archetype, mut tail) in columns {
        let mut consumed = 0;
        while let Some(&((target, row), output)) = requested.peek() {
            if target < archetype {
                return None;
            }
            if target > archetype {
                break;
            }
            let (_, suffix) = tail.split_at_mut_checked(row - consumed)?;
            let (value, suffix) = suffix.split_first_mut()?;
            result[output] = Some(value);
            tail = suffix;
            consumed = row + 1;
            requested.next();
        }
        if requested.peek().is_none() {
            break;
        }
    }
    if requested.next().is_some() {
        return None;
    }
    Some(result.map(|value| value.expect("every requested row was visited")))
}

#[derive(Default)]
pub(crate) struct SparseIndex {
    rows: HashMap<Entity, usize>,
    entities: Vec<Entity>,
}

impl SparseIndex {
    pub(crate) fn get(&self, entity: Entity) -> Option<usize> {
        self.rows.get(&entity).copied()
    }

    fn remove(&mut self, entity: Entity) -> Option<usize> {
        let row = self.rows.remove(&entity)?;
        self.entities.swap_remove(row);
        if let Some(&swapped) = self.entities.get(row) {
            self.rows.insert(swapped, row);
        }
        Some(row)
    }
}
