//! Runtime type metadata shared by authoring and the ECS (ADR 0003).

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;

/// A type that can describe its schema without constructing a value.
///
/// Derive this trait for structs and enums. Use `#[reflect(version = 2)]` to
/// override the initial schema version of 1. Types, variants and fields can
/// have a `display_name` and `tooltip`. Fields also accept `range = -1.0..=1.0`.
/// These editor attributes are validated in every build, and their data is
/// emitted only with the `editor-metadata` feature.
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// #[reflect(renamed = "Position")]
/// struct Invalid;
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// #[reflect(version = 1, version = 2)]
/// struct Invalid;
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// struct Invalid(#[reflect(version = 2)] u32);
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// struct Invalid(#[reflect(range = "zero"..=1)] f64);
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// struct Invalid(#[reflect(range = 0..=1e999)] f64);
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// struct Invalid(#[reflect(range = 2..=1)] f64);
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// struct Invalid(#[reflect(range = 0..1)] f64);
/// ```
///
/// ```compile_fail
/// # use engine_core::Reflect;
/// #[derive(Reflect)]
/// union Invalid { value: u32 }
/// ```
pub trait Reflect: 'static {
    /// Describes the fields or variants and the version of this type's schema.
    fn schema() -> TypeSchema;
}

/// A key assigned in registration order within one [`TypeRegistry`].
///
/// Its identity is local to the registry. It is suitable for runtime indexing,
/// while persisted data needs its own stable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeKey(usize);

/// Registered types in the order they first entered the registry.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    keys: HashMap<TypeId, TypeKey>,
    entries: Vec<TypeRegistration>,
}

impl TypeRegistry {
    /// Registers a type that may have runtime metadata without a schema.
    /// Repeated registrations return the original entry.
    pub fn register<T: 'static>(&mut self) -> &mut TypeRegistration {
        let type_id = TypeId::of::<T>();
        let key = *self.keys.entry(type_id).or_insert_with(|| {
            let key = TypeKey(self.entries.len());
            self.entries.push(TypeRegistration {
                key,
                type_id,
                name: type_name::<T>(),
                schema: None,
                extensions: HashMap::new(),
            });
            key
        });
        &mut self.entries[key.0]
    }

    /// Registers a reflected type, adding its schema to any existing entry.
    pub fn register_reflect<T: Reflect>(&mut self) -> &mut TypeRegistration {
        let entry = self.register::<T>();
        entry.schema.get_or_insert_with(T::schema);
        entry
    }

    /// Finds a registration by its Rust type.
    #[must_use]
    pub fn get<T: 'static>(&self) -> Option<&TypeRegistration> {
        self.get_by_type_id(TypeId::of::<T>())
    }

    /// Finds a registration from a reflected field's type identifier.
    #[must_use]
    pub fn get_by_type_id(&self, type_id: TypeId) -> Option<&TypeRegistration> {
        self.keys
            .get(&type_id)
            .and_then(|key| self.get_by_key(*key))
    }

    /// Looks up a key previously obtained from this registry.
    #[must_use]
    pub fn get_by_key(&self, key: TypeKey) -> Option<&TypeRegistration> {
        self.entries.get(key.0)
    }

    /// Visits registrations in insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &TypeRegistration> {
        self.entries.iter()
    }
}

/// The schema and subsystem metadata associated with one Rust type.
#[derive(Debug)]
pub struct TypeRegistration {
    key: TypeKey,
    type_id: TypeId,
    name: &'static str,
    schema: Option<TypeSchema>,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl TypeRegistration {
    /// This type's registry-local key.
    #[must_use]
    pub fn key(&self) -> TypeKey {
        self.key
    }

    /// This type's process-local Rust identity.
    #[must_use]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Rust type name for diagnostics. Its spelling can change between builds.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Schema, if the type was registered through [`TypeRegistry::register_reflect`].
    #[must_use]
    pub fn schema(&self) -> Option<&TypeSchema> {
        self.schema.as_ref()
    }

    /// Attaches subsystem-specific metadata, replacing the previous value of
    /// the same metadata type if present.
    pub fn insert_metadata<M: Send + Sync + 'static>(&mut self, metadata: M) {
        self.extensions
            .insert(TypeId::of::<M>(), Box::new(metadata));
    }

    /// Looks up subsystem-specific metadata on this registration.
    #[must_use]
    pub fn metadata<M: Send + Sync + 'static>(&self) -> Option<&M> {
        self.extensions.get(&TypeId::of::<M>())?.downcast_ref()
    }
}

/// A type's reflected shape and schema version.
#[derive(Debug)]
pub struct TypeSchema {
    /// Version of this type's serialized schema.
    pub version: u32,
    /// Fields or variants in declaration order.
    pub shape: TypeShape,
    /// Optional labels for authoring tools.
    #[cfg(feature = "editor-metadata")]
    pub editor: EditorMetadata,
}

impl TypeSchema {
    /// Creates a schema with no editor metadata.
    #[must_use]
    pub fn new(version: u32, shape: TypeShape) -> Self {
        Self {
            version,
            shape,
            #[cfg(feature = "editor-metadata")]
            editor: EditorMetadata::default(),
        }
    }
}

/// The fields of a struct or the variants of an enum.
#[derive(Debug)]
pub enum TypeShape {
    /// Named fields, positional fields, or an empty list for a unit struct.
    Struct(Box<[Field]>),
    /// The variants in source order.
    Enum(Box<[Variant]>),
}

/// A named or positional field in declaration order.
#[derive(Debug)]
pub struct Field {
    /// Field name, or its decimal index for a tuple field.
    pub name: &'static str,
    /// Rust identity used to look up this field's type in the registry.
    pub type_id: TypeId,
    /// Rust type name for diagnostics.
    pub type_name: &'static str,
    /// Optional labels and range for authoring tools.
    #[cfg(feature = "editor-metadata")]
    pub editor: EditorMetadata,
}

impl Field {
    /// Creates metadata for a field of type `T`.
    #[must_use]
    pub fn of<T: 'static>(name: &'static str) -> Self {
        Self {
            name,
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
            #[cfg(feature = "editor-metadata")]
            editor: EditorMetadata::default(),
        }
    }
}

/// One enum variant and the shape of its payload.
#[derive(Debug)]
pub struct Variant {
    /// The declared variant name.
    pub name: &'static str,
    /// Named or positional fields, empty for a unit variant.
    pub fields: Box<[Field]>,
    /// Optional labels for authoring tools.
    #[cfg(feature = "editor-metadata")]
    pub editor: EditorMetadata,
}

impl Variant {
    /// Creates metadata for an enum variant and its fields.
    #[must_use]
    pub fn new(name: &'static str, fields: Box<[Field]>) -> Self {
        Self {
            name,
            fields,
            #[cfg(feature = "editor-metadata")]
            editor: EditorMetadata::default(),
        }
    }
}

/// Metadata available to the inspector when the editor feature is enabled.
#[cfg(feature = "editor-metadata")]
#[derive(Debug, Default)]
pub struct EditorMetadata {
    /// Label shown in place of the Rust name.
    pub display_name: Option<&'static str>,
    /// Help shown when inspecting a type, variant or field.
    pub tooltip: Option<&'static str>,
    /// Inclusive lower and upper bounds for editing a numeric field.
    pub range: Option<(f64, f64)>,
}
