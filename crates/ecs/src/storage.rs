//! Type-owned value storage. Each table column is a contiguous `Vec<T>`.

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

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
    pub(crate) fn get<T: Component>(&self, key: TypeKey) -> Option<&Storage<T>> {
        self.0.get(&key)?.as_any().downcast_ref()
    }

    pub(crate) fn get_mut<T: Component>(&mut self, key: TypeKey) -> Option<&mut Storage<T>> {
        self.0.get_mut(&key)?.as_any_mut().downcast_mut()
    }

    pub(crate) fn ensure<T: Component>(&mut self, key: TypeKey) -> &mut Storage<T> {
        self.0
            .entry(key)
            .or_insert_with(|| {
                let store = match T::STORAGE {
                    StorageKind::Table => Storage::Table(HashMap::<usize, Vec<T>>::new()),
                    StorageKind::Sparse => Storage::Sparse(SparseSet::<T>::new()),
                };
                Box::new(store)
            })
            .as_any_mut()
            .downcast_mut()
            .expect("component key has one Rust type")
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
    fn move_row(&mut self, from: Location, to: usize);
    fn take_row(&mut self, location: Location) -> Box<dyn Any>;
    fn take_sparse(&mut self, entity: Entity) -> Option<Box<dyn Any>>;
}

pub(crate) enum Storage<T> {
    Table(HashMap<usize, Vec<T>>),
    Sparse(SparseSet<T>),
}

impl<T: Component> Storage<T> {
    pub(crate) fn get(&self, entity: Entity, location: Location) -> Option<&T> {
        match self {
            Self::Table(columns) => columns.get(&location.archetype)?.get(location.row),
            Self::Sparse(set) => Some(&set.dense[*set.rows.get(&entity)?].1),
        }
    }

    pub(crate) fn get_mut(&mut self, entity: Entity, location: Location) -> Option<&mut T> {
        match self {
            Self::Table(columns) => columns.get_mut(&location.archetype)?.get_mut(location.row),
            Self::Sparse(set) => Some(&mut set.dense[*set.rows.get(&entity)?].1),
        }
    }

    pub(crate) fn table_mut(&mut self, archetype: usize) -> &mut Vec<T> {
        let Self::Table(columns) = self else {
            unreachable!("table component storage")
        };
        columns.entry(archetype).or_default()
    }

    pub(crate) fn take(&mut self, entity: Entity, location: Location) -> Option<T> {
        match self {
            Self::Table(columns) => Some(
                columns
                    .get_mut(&location.archetype)?
                    .swap_remove(location.row),
            ),
            Self::Sparse(set) => set.remove(entity),
        }
    }
}

impl<T: Component> ErasedStorage for Storage<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn move_row(&mut self, from: Location, to: usize) {
        let value = self.table_mut(from.archetype).swap_remove(from.row);
        self.table_mut(to).push(value);
    }

    fn take_row(&mut self, location: Location) -> Box<dyn Any> {
        Box::new(self.table_mut(location.archetype).swap_remove(location.row))
    }

    fn take_sparse(&mut self, entity: Entity) -> Option<Box<dyn Any>> {
        let Self::Sparse(set) = self else { return None };
        set.remove(entity)
            .map(|value| Box::new(value) as Box<dyn Any>)
    }
}

pub(crate) struct SparseSet<T> {
    rows: HashMap<Entity, usize>,
    dense: Vec<(Entity, T)>,
}

impl<T> SparseSet<T> {
    fn new() -> Self {
        Self {
            rows: HashMap::new(),
            dense: Vec::new(),
        }
    }

    pub(crate) fn insert(&mut self, entity: Entity, value: T) -> Option<T> {
        if let Some(&row) = self.rows.get(&entity) {
            return Some(std::mem::replace(&mut self.dense[row].1, value));
        }
        self.rows.insert(entity, self.dense.len());
        self.dense.push((entity, value));
        None
    }

    fn remove(&mut self, entity: Entity) -> Option<T> {
        let row = self.rows.remove(&entity)?;
        let (_, value) = self.dense.swap_remove(row);
        if let Some((swapped, _)) = self.dense.get(row) {
            self.rows.insert(*swapped, row);
        }
        Some(value)
    }
}
