use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Scoped data with one value per Rust type.
#[derive(Debug, Default)]
pub struct Resources(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl Resources {
    /// Inserts a resource, returning the previously stored value of its type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.0
            .insert(TypeId::of::<T>(), Box::new(value))
            .map(|old| *old.downcast::<T>().expect("resource key has one Rust type"))
    }

    /// Borrows the resource of type `T`, if one has been inserted.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Mutably borrows the resource of type `T`.
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }
}
