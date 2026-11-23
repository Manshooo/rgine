use std::any::TypeId;

use crate::{Commands, Entity, World};

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
/// Use `#[component(requires(TypeA, TypeB), on_add = path, on_remove = path)]`
/// to declare requirements or hooks with the derive. Each option is independent.
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

    /// Default components inserted when missing, in declaration order.
    const REQUIRED: &'static [RequiredComponent] = &[];

    /// Called after a new component and its requirements have been inserted.
    const ON_ADD: Option<ComponentHook> = None;

    /// Called after the component has been removed, including on despawn.
    const ON_REMOVE: Option<ComponentHook> = None;
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

/// Runs after a component is added or removed. Use the supplied
/// [`Commands`] to request further structural changes.
pub type ComponentHook = fn(&World, Entity, &mut Commands);

/// A component type that can be inserted with its default value.
#[derive(Debug, Clone, Copy)]
pub struct RequiredComponent {
    type_id: fn() -> TypeId,
    pub(crate) install: fn(&mut World, Entity, &mut Vec<ComponentHook>) -> &'static [Self],
}

impl RequiredComponent {
    /// Declares a requirement on the default of `T`.
    #[must_use]
    pub const fn new<T: Component + Default>() -> Self {
        Self {
            type_id: TypeId::of::<T>,
            install: World::insert_required::<T>,
        }
    }

    /// Returns the Rust type identity of the required component.
    #[must_use]
    pub fn type_id(self) -> TypeId {
        (self.type_id)()
    }
}

/// Requirements and hooks on the type registration of a component that
/// declares them.
#[derive(Debug, Clone, Copy)]
pub struct ComponentLifecycle {
    /// The component's direct requirements in declaration order.
    pub required: &'static [RequiredComponent],
    /// The hook for a new component.
    pub on_add: Option<ComponentHook>,
    /// The hook for a removed component.
    pub on_remove: Option<ComponentHook>,
}
