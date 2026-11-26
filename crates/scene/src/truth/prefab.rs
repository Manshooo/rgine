use std::{
    any::{TypeId, type_name},
    collections::{BTreeMap, BTreeSet},
    ptr,
};

use engine_core::{
    Reflect, TypeRegistry,
    reflection::{Field, TypeKey},
};
use ron::value::RawValue;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

use super::{Change, ObjectId, PropertyValue, SceneTypeName, Truth, TruthError, TruthObject};

const PLACEMENT: &str = "placement";

#[derive(Reflect)]
struct PrefabNode {
    #[allow(dead_code)]
    placement: PrefabInstance,
}

/// Registers the built-in prefab placement type for scene authoring and loading.
pub fn register_prefabs(registry: &mut TypeRegistry) {
    registry
        .register_reflect::<PrefabNode>()
        .insert_metadata(SceneTypeName("rgine.prefab_instance"));
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct Overrides(
    #[serde(deserialize_with = "super::persistence::unique_map")] BTreeMap<String, Box<RawValue>>,
);

/// A linked prefab and the properties edited locally on this placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabInstance {
    asset: Uuid,
    root: ObjectId,
    #[serde(deserialize_with = "super::persistence::unique_map")]
    overrides: BTreeMap<ObjectId, Overrides>,
}

impl PrefabInstance {
    /// Links a prefab asset GUID and the stable root ID in that asset.
    pub fn new(asset: Uuid, root: ObjectId) -> Self {
        Self {
            asset,
            root,
            overrides: BTreeMap::new(),
        }
    }

    /// Returns the GUID supplied to the source lookup during resolution.
    pub fn asset(&self) -> Uuid {
        self.asset
    }

    /// Returns the local root ID expected in the prefab source.
    pub fn root(&self) -> ObjectId {
        self.root
    }

    /// Returns the scene ID for a source node, using the placement ID for the root.
    /// The caller can use this ID when authoring a reference to a node in this instance.
    pub fn node_id(&self, instance: ObjectId, source_node: ObjectId) -> ObjectId {
        if source_node == self.root {
            instance
        } else {
            ObjectId(Uuid::new_v5(&instance.0, source_node.0.as_bytes()))
        }
    }

    /// Sets an override after checking the field and value type against the source.
    /// Object IDs in override values are in scene space.
    pub fn set_override<T: Serialize + DeserializeOwned + 'static>(
        &mut self,
        source: &Truth<'_>,
        node: ObjectId,
        name: &str,
        value: &T,
    ) -> Result<(), TruthError> {
        let (field, data) = source.encode_property(node, name, value)?;
        self.overrides
            .entry(node)
            .or_default()
            .0
            .insert(field.name.to_owned(), data);
        Ok(())
    }

    /// Resumes inheritance for one locally overridden property.
    pub fn clear_override(&mut self, node: ObjectId, name: &str) {
        if let Some(fields) = self.overrides.get_mut(&node) {
            fields.0.remove(name);
            if fields.0.is_empty() {
                self.overrides.remove(&node);
            }
        }
    }
}

/// Why an authored prefab placement could not be read, edited or resolved.
#[derive(Debug, thiserror::Error)]
pub enum PrefabError {
    /// The supplied object ID does not belong to a prefab placement.
    #[error("authored object {0} is not a prefab placement")]
    NotAnInstance(ObjectId),
    /// The placement field is missing or unreadable.
    #[error("prefab instance {instance} has invalid placement data: {source}")]
    InvalidPlacement {
        /// Offending placement.
        instance: ObjectId,
        /// Property access or decoding error.
        source: Box<TruthError>,
    },
    /// The source lookup did not provide the asset.
    #[error("prefab instance {instance} refers to missing asset {asset}")]
    MissingAsset {
        /// Offending placement.
        instance: ObjectId,
        /// Prefab asset GUID.
        asset: Uuid,
    },
    /// Source objects must use the same type registry as the containing scene.
    #[error("prefab instance {0} refers to a source from a different type registry")]
    RegistryMismatch(ObjectId),
    /// The source lacks the expected single root.
    #[error("prefab instance {instance} asset {asset} must have {root} as its only root")]
    InvalidRoot {
        /// Offending placement.
        instance: ObjectId,
        /// Prefab asset GUID.
        asset: Uuid,
        /// Root expected by the placement.
        root: ObjectId,
    },
    /// A prefab source contains another linked placement.
    #[error("prefab instance {instance} contains nested prefab placement {node}")]
    Nested {
        /// Placement being resolved.
        instance: ObjectId,
        /// Nested prefab placement in the source.
        node: ObjectId,
    },
    /// The instance's derived ID would replace another scene object.
    #[error("prefab instance {instance} would reuse authored object ID {object}")]
    IdCollision {
        /// Offending placement.
        instance: ObjectId,
        /// Conflicting scene ID.
        object: ObjectId,
    },
    /// A local override refers to a deleted or nonexistent source object.
    #[error("prefab instance {instance} overrides missing source node {node}")]
    MissingOverrideNode {
        /// Offending placement.
        instance: ObjectId,
        /// Missing source node.
        node: ObjectId,
    },
    /// An inherited object reference points outside the source.
    #[error(
        "prefab instance {instance} source node {node} field {name:?} refers to missing source node {target}"
    )]
    MissingTarget {
        /// Offending placement.
        instance: ObjectId,
        /// Node with the inherited reference.
        node: ObjectId,
        /// Field containing the reference.
        name: &'static str,
        /// Missing source node.
        target: ObjectId,
    },
    /// A property in the inherited or overridden data is invalid.
    #[error("prefab instance {instance} source node {node} field {name:?}: {source}")]
    InvalidProperty {
        /// Offending placement.
        instance: ObjectId,
        /// Node with the invalid field.
        node: ObjectId,
        /// Field with the property error.
        name: String,
        /// Property access, type or decoding error.
        source: Box<TruthError>,
    },
}

impl<'registry> Truth<'registry> {
    /// Places a prefab root at an authored ID and records a single journal entry.
    pub fn create_prefab(
        &mut self,
        id: ObjectId,
        parent: Option<ObjectId>,
        instance: PrefabInstance,
    ) -> Result<(), TruthError> {
        self.validate_new_object(id, parent)?;
        let type_key = self
            .prefab_type()
            .ok_or(TruthError::UnregisteredType(type_name::<PrefabNode>()))?;
        let field = Field::of::<PrefabInstance>(PLACEMENT);
        let placement = PropertyValue::from_raw(&field, RawValue::from_rust(&instance)?);
        self.insert_object(
            id,
            type_key,
            parent,
            BTreeMap::from([(PLACEMENT, placement)]),
        );
        self.record(id, Change::PrefabCreated { parent, instance });
        Ok(())
    }

    /// Reads the linked source and overrides of an authored prefab placement.
    pub fn prefab(&self, id: ObjectId) -> Result<PrefabInstance, PrefabError> {
        let object = self.require_prefab(id)?;
        let invalid = |source| PrefabError::InvalidPlacement {
            instance: id,
            source: Box::new(source),
        };
        object
            .property(PLACEMENT)
            .ok_or_else(|| {
                invalid(TruthError::UnknownProperty {
                    object: id,
                    name: PLACEMENT.to_owned(),
                })
            })?
            .read()
            .map_err(invalid)
    }

    /// Updates the link and overrides as one journaled property edit.
    pub fn update_prefab(
        &mut self,
        id: ObjectId,
        instance: &PrefabInstance,
    ) -> Result<(), PrefabError> {
        self.require_prefab(id)?;
        self.set_property(id, PLACEMENT, instance)
            .map_err(|source| PrefabError::InvalidPlacement {
                instance: id,
                source: Box::new(source),
            })
    }

    /// Expands all links into a snapshot that can be passed to the scene compiler.
    /// The source callback resolves asset GUIDs to single-root Truth documents using this registry.
    /// The resulting snapshot starts with an empty journal.
    pub fn resolve_prefabs<'source>(
        &self,
        mut lookup: impl FnMut(Uuid) -> Option<&'source Truth<'registry>>,
    ) -> Result<Self, PrefabError>
    where
        'registry: 'source,
    {
        let mut resolved = Self {
            registry: self.registry,
            objects: self.objects.clone(),
            journal: Vec::new(),
        };
        let Some(prefab_type) = self.prefab_type() else {
            return Ok(resolved);
        };
        for (id, placement) in self
            .objects()
            .filter(|(_, object)| object.type_key == prefab_type)
        {
            let prefab = self.prefab(id)?;
            let source = lookup(prefab.asset).ok_or(PrefabError::MissingAsset {
                instance: id,
                asset: prefab.asset,
            })?;
            resolved.expand_prefab(id, placement, &prefab, source)?;
        }
        Ok(resolved)
    }

    pub(crate) fn first_prefab(&self) -> Option<ObjectId> {
        let prefab_type = self.prefab_type()?;
        self.objects()
            .find(|(_, object)| object.type_key == prefab_type)
            .map(|(id, _)| id)
    }

    fn prefab_type(&self) -> Option<TypeKey> {
        self.registry
            .get::<PrefabNode>()
            .map(|registration| registration.key())
    }

    fn require_prefab(&self, id: ObjectId) -> Result<&TruthObject, PrefabError> {
        self.object(id)
            .filter(|object| Some(object.type_key) == self.prefab_type())
            .ok_or(PrefabError::NotAnInstance(id))
    }

    fn expand_prefab(
        &mut self,
        id: ObjectId,
        placement: &TruthObject,
        prefab: &PrefabInstance,
        source: &Truth<'registry>,
    ) -> Result<(), PrefabError> {
        if !ptr::eq(self.registry, source.registry) {
            return Err(PrefabError::RegistryMismatch(id));
        }
        let mut roots = source
            .objects()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(id, _)| id);
        if roots.next() != Some(prefab.root) || roots.next().is_some() {
            return Err(PrefabError::InvalidRoot {
                instance: id,
                asset: prefab.asset,
                root: prefab.root,
            });
        }
        if let Some(node) = source.first_prefab() {
            return Err(PrefabError::Nested { instance: id, node });
        }
        for &node in prefab.overrides.keys() {
            if source.object(node).is_none() {
                return Err(PrefabError::MissingOverrideNode { instance: id, node });
            }
        }
        let remap: BTreeMap<_, _> = source
            .objects()
            .map(|(node, _)| (node, prefab.node_id(id, node)))
            .collect();
        for (node, template) in source.objects() {
            let object = remap[&node];
            let mut children: BTreeSet<_> =
                template.children().map(|child| remap[&child]).collect();
            let parent = if node == prefab.root {
                children.extend(placement.children());
                placement.parent
            } else {
                if self.objects.contains_key(&object) {
                    return Err(PrefabError::IdCollision {
                        instance: id,
                        object,
                    });
                }
                template.parent.map(|parent| remap[&parent])
            };
            let properties = resolve_properties(
                id,
                node,
                template,
                source,
                prefab.overrides.get(&node),
                &remap,
            )?;
            self.objects.insert(
                object,
                TruthObject {
                    parent,
                    children,
                    properties,
                    type_key: template.type_key,
                },
            );
        }
        Ok(())
    }
}

fn resolve_properties(
    instance: ObjectId,
    node: ObjectId,
    template: &TruthObject,
    source: &Truth<'_>,
    overrides: Option<&Overrides>,
    remap: &BTreeMap<ObjectId, ObjectId>,
) -> Result<BTreeMap<&'static str, PropertyValue>, PrefabError> {
    let invalid = |name: &str, source| PrefabError::InvalidProperty {
        instance,
        node,
        name: name.to_owned(),
        source: Box::new(source),
    };
    let mut properties = BTreeMap::new();
    for (name, value) in template.properties() {
        if overrides.is_some_and(|fields| fields.0.contains_key(name)) {
            continue;
        }
        let mapped = |target| {
            remap
                .get(&target)
                .copied()
                .ok_or(PrefabError::MissingTarget {
                    instance,
                    node,
                    name,
                    target,
                })
        };
        let data = if value.type_id == TypeId::of::<ObjectId>() {
            let target = value
                .read::<ObjectId>()
                .map_err(|error| invalid(name, error))?;
            RawValue::from_rust(&mapped(target)?)
        } else if value.type_id == TypeId::of::<Option<ObjectId>>() {
            let target = value
                .read::<Option<ObjectId>>()
                .map_err(|error| invalid(name, error))?;
            RawValue::from_rust(&target.map(mapped).transpose()?)
        } else {
            properties.insert(name, value.clone());
            continue;
        };
        let field = source
            .field(node, name)
            .map_err(|error| invalid(name, error))?;
        let data = data.map_err(|error| invalid(name, error.into()))?;
        properties.insert(name, PropertyValue::from_raw(field, data));
    }
    if let Some(overrides) = overrides {
        for (name, value) in &overrides.0 {
            let field = source
                .field(node, name)
                .map_err(|error| invalid(name, error))?;
            properties.insert(
                field.name,
                PropertyValue::from_raw(field, value.clone().trim_boxed()),
            );
        }
    }
    Ok(properties)
}
