/// A type whose values can be attached to entities.
///
/// `#[derive(Component)]` uses table storage. `#[component(storage = "sparse")]`
/// opts into sparse storage for markers or types that are toggled often.
/// Components can contain owned data or static references, and a world can be
/// moved between threads with its components.
///
/// ```
/// use engine_ecs::{Component, World};
///
/// #[derive(Component, Debug)]
/// struct Health(u32);
///
/// let mut world = World::new();
/// let entity = world.spawn_empty();
/// world.insert(entity, Health(100)).unwrap();
/// assert_eq!(world.get::<Health>(entity).unwrap().0, 100);
/// ```
///
/// ```compile_fail
/// # use engine_ecs::Component;
/// #[derive(Component)]
/// #[component(storage = "packed")]
/// struct Invalid;
/// ```
///
/// ```compile_fail
/// # use engine_ecs::Component;
/// #[derive(Component)]
/// #[component(storage = "table", storage = "sparse")]
/// struct Invalid;
/// ```
///
/// ```compile_fail
/// # use engine_ecs::Component;
/// #[derive(Component)]
/// #[component(store = "sparse")]
/// struct Invalid;
/// ```
///
/// ```compile_fail
/// # use engine_ecs::Component;
/// #[derive(Component)]
/// struct Invalid(#[component(storage = "sparse")] u32);
/// ```
pub trait Component: Send + Sync + 'static {
    /// How this component type is stored in a [`World`](crate::World).
    const STORAGE: StorageKind = StorageKind::Table;
}

/// The storage strategy shared by every instance of a component type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    /// Values occupy a dense column in each archetype that has the component.
    Table,
    /// Values live in a separate set and can change without moving the entity.
    Sparse,
}

/// ECS metadata on a type's registration in the shared type registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentMetadata {
    /// The component type's storage strategy.
    pub storage: StorageKind,
}
