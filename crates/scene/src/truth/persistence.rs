use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use engine_core::{TypeRegistry, reflection::TypeRegistration};
use ron::{ser::PrettyConfig, value::RawValue};
use serde::{Deserialize, Deserializer, Serialize, de};

use super::{ObjectId, PropertyValue, Truth, TruthError, TruthObject, TypeShape};

const SCENE_VERSION: u32 = 1;

/// Stable persisted name for a reflected struct used in a scene.
///
/// Attach this metadata to its registration using
/// `registry.register_reflect::<T>().insert_metadata(SceneTypeName("game.node"))`.
/// Names must be unique in the registry and stay the same when Rust types move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTypeName(pub &'static str);

/// A scene could not be encoded or restored.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// The scene format version has no reader.
    #[error("unsupported scene schema version {0}; expected {SCENE_VERSION}")]
    UnsupportedVersion(u32),
    /// The RON document does not match the scene format.
    #[error("could not read scene RON: {0}")]
    Decode(#[from] ron::error::SpannedError),
    /// RON serialization failed.
    #[error("could not write scene RON: {0}")]
    Encode(#[from] ron::Error),
    /// Two registry entries use the same persisted name.
    #[error("scene type name {0:?} is registered more than once")]
    DuplicateTypeName(&'static str),
    /// An object uses a reflected type without a persisted name.
    #[error("authoring type {0} has no registered scene type name")]
    MissingTypeName(&'static str),
    /// The document uses a name absent from the registry.
    #[error("authored object {object} uses unknown scene type {name:?}")]
    UnknownType {
        /// Object whose type could not be resolved.
        object: ObjectId,
        /// Persisted type name.
        name: String,
    },
    /// The object needs a version of its registered schema with no reader.
    #[error(
        "authored object {object} of type {name:?} has schema version {found}; expected {expected}"
    )]
    UnsupportedTypeVersion {
        /// Object with the unsupported type version.
        object: ObjectId,
        /// Registered scene type name.
        name: &'static str,
        /// Version stored on disk.
        found: u32,
        /// Version registered by the authoring application.
        expected: u32,
    },
    /// A parent edge points outside the scene.
    #[error("authored object {object} refers to missing parent {parent}")]
    MissingParent {
        /// Object with the dangling parent edge.
        object: ObjectId,
        /// Missing parent ID.
        parent: ObjectId,
    },
    /// An object cannot be reached from a root because of a parent cycle.
    #[error("authored object {0} is in a cyclic hierarchy")]
    CyclicHierarchy(ObjectId),
    /// The document uses a type or field unsupported by Truth.
    #[error(transparent)]
    InvalidObject(#[from] TruthError),
}

#[derive(Deserialize)]
struct VersionHeader {
    schema_version: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneFile<T> {
    schema_version: u32,
    #[serde(
        deserialize_with = "unique_map",
        bound(deserialize = "T: Deserialize<'de>")
    )]
    objects: BTreeMap<ObjectId, SavedObject<T>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedObject<T> {
    type_name: String,
    type_version: u32,
    parent: Option<ObjectId>,
    #[serde(
        deserialize_with = "unique_map",
        bound(deserialize = "T: Deserialize<'de>")
    )]
    properties: BTreeMap<String, T>,
}

impl<'registry> Truth<'registry> {
    /// Writes the current authoring snapshot as UTF-8 RON ordered by object ID and field name.
    /// Every object type must have a registered [`SceneTypeName`].
    pub fn to_ron(&self) -> Result<String, SceneError> {
        scene_types(self.registry)?;
        let mut objects = BTreeMap::new();
        for (id, object) in self.objects() {
            let registration = self
                .registry
                .get_by_key(object.type_key)
                .expect("registered object type");
            let name = scene_type_name(registration)
                .ok_or_else(|| SceneError::MissingTypeName(registration.name()))?;
            objects.insert(
                id,
                SavedObject {
                    type_name: name.to_owned(),
                    type_version: registration
                        .schema()
                        .expect("registered object schema")
                        .version,
                    parent: object.parent,
                    properties: object
                        .properties()
                        .map(|(name, value)| (name.to_owned(), value.data.as_ref()))
                        .collect(),
                },
            );
        }
        let scene = SceneFile {
            schema_version: SCENE_VERSION,
            objects,
        };
        let mut text = ron::ser::to_string_pretty(&scene, PrettyConfig::default().new_line("\n"))?;
        text.push('\n');
        Ok(text)
    }

    /// Reads a RON authoring snapshot against the supplied registry.
    ///
    /// Checks versions, types, authored field names and the parent hierarchy. Property fragments
    /// are decoded as their declared Rust types when read. The loaded scene starts a new journal
    /// and preserves authored object and asset references, including unresolved ones.
    pub fn from_ron(registry: &'registry TypeRegistry, text: &str) -> Result<Self, SceneError> {
        // Read the version independently so a future layout can still report its version.
        let VersionHeader { schema_version } = ron::from_str(text)?;
        if schema_version != SCENE_VERSION {
            return Err(SceneError::UnsupportedVersion(schema_version));
        }
        let scene: SceneFile<Box<RawValue>> = ron::from_str(text)?;
        let types = scene_types(registry)?;
        let mut truth = Self::new(registry);
        for (id, saved) in scene.objects {
            let Some((&name, &registration)) = types.get_key_value(saved.type_name.as_str()) else {
                return Err(SceneError::UnknownType {
                    object: id,
                    name: saved.type_name,
                });
            };
            let Some(schema) = registration.schema() else {
                return Err(TruthError::UnregisteredType(registration.name()).into());
            };
            if saved.type_version != schema.version {
                return Err(SceneError::UnsupportedTypeVersion {
                    object: id,
                    name,
                    found: saved.type_version,
                    expected: schema.version,
                });
            }
            if !matches!(schema.shape, TypeShape::Struct(_)) {
                return Err(TruthError::UnsupportedType(registration.name()).into());
            }
            truth.objects.insert(
                id,
                TruthObject {
                    type_key: registration.key(),
                    parent: saved.parent,
                    children: BTreeSet::new(),
                    properties: BTreeMap::new(),
                },
            );
            for (name, data) in saved.properties {
                let field = truth.field(id, &name)?;
                truth.replace_property(
                    id,
                    field.name,
                    Some(PropertyValue::from_raw(field, data.trim_boxed())),
                );
            }
        }
        truth.restore_hierarchy()?;
        Ok(truth)
    }

    fn restore_hierarchy(&mut self) -> Result<(), SceneError> {
        let edges: Vec<_> = self.objects().map(|(id, node)| (id, node.parent)).collect();
        let mut pending = Vec::new();
        for (object, parent) in edges {
            if let Some(parent) = parent {
                if !self.objects.contains_key(&parent) {
                    return Err(SceneError::MissingParent { object, parent });
                }
                self.attach(object, Some(parent));
            } else {
                pending.push(object);
            }
        }
        // With one parent per node, only cyclic components are unreachable from roots.
        let mut unvisited: BTreeSet<_> = self.objects.keys().copied().collect();
        while let Some(id) = pending.pop() {
            unvisited.remove(&id);
            pending.extend(self.objects[&id].children());
        }
        if let Some(id) = unvisited.first() {
            return Err(SceneError::CyclicHierarchy(*id));
        }
        Ok(())
    }
}

fn scene_type_name(registration: &TypeRegistration) -> Option<&'static str> {
    registration.metadata::<SceneTypeName>().map(|name| name.0)
}

fn scene_types(
    registry: &TypeRegistry,
) -> Result<BTreeMap<&'static str, &TypeRegistration>, SceneError> {
    let mut types = BTreeMap::new();
    for registration in registry.iter() {
        if let Some(name) = scene_type_name(registration) {
            if types.insert(name, registration).is_some() {
                return Err(SceneError::DuplicateTypeName(name));
            }
        }
    }
    Ok(types)
}

pub(super) fn unique_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Ord + fmt::Debug,
    V: Deserialize<'de>,
{
    struct Unique<K, V>(PhantomData<(K, V)>);

    impl<'de, K, V> de::Visitor<'de> for Unique<K, V>
    where
        K: Deserialize<'de> + Ord + fmt::Debug,
        V: Deserialize<'de>,
    {
        type Value = BTreeMap<K, V>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a map with unique keys")
        }

        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some((key, value)) = map.next_entry()? {
                match entries.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        return Err(de::Error::custom(format!(
                            "duplicate key {:?}",
                            entry.key()
                        )));
                    }
                }
            }
            Ok(entries)
        }
    }

    deserializer.deserialize_map(Unique(PhantomData))
}
