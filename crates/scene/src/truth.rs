use std::{
    any::{TypeId, type_name},
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use engine_core::{
    Reflect, TypeRegistry,
    reflection::{Field, TypeKey, TypeShape},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

/// Stable GUID of an authored object within its scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(Uuid);

impl ObjectId {
    /// Generates an ID for a new authored object.
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }

    /// Reconstructs an ID from its UUID bits.
    pub const fn from_u128(bits: u128) -> Self {
        Self(Uuid::from_u128(bits))
    }

    /// Returns the object's UUID bits.
    pub const fn as_u128(self) -> u128 {
        self.0.as_u128()
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An owned snapshot of a reflected property's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyValue {
    type_id: TypeId,
    type_name: &'static str,
    // A RON fragment snapshots nested values without sharing their mutable state.
    data: Arc<str>,
}

impl PropertyValue {
    /// Reads the snapshot as the field's original Rust type.
    pub fn read<T: DeserializeOwned + 'static>(&self) -> Result<T, TruthError> {
        if self.type_id != TypeId::of::<T>() {
            return Err(TruthError::PropertyType {
                expected: self.type_name,
                actual: type_name::<T>(),
            });
        }
        Ok(ron::from_str(&self.data)?)
    }
}

/// One object in Truth, with optional field values from its registered struct.
#[derive(Debug, PartialEq, Eq)]
pub struct TruthObject {
    type_key: TypeKey,
    parent: Option<ObjectId>,
    children: BTreeSet<ObjectId>,
    properties: BTreeMap<&'static str, PropertyValue>,
}

impl TruthObject {
    /// Type key in the registry supplied to Truth.
    pub fn type_key(&self) -> TypeKey {
        self.type_key
    }

    /// Parent in the same scene, if this object has one.
    pub fn parent(&self) -> Option<ObjectId> {
        self.parent
    }

    /// Direct children in stable ID order.
    pub fn children(&self) -> impl ExactSizeIterator<Item = ObjectId> + '_ {
        self.children.iter().copied()
    }

    /// Looks up an authored field value. Unset fields return `None`.
    pub fn property(&self, name: &str) -> Option<&PropertyValue> {
        self.properties.get(name)
    }

    /// Visits authored property values in field name order.
    pub fn properties(&self) -> impl ExactSizeIterator<Item = (&'static str, &PropertyValue)> {
        self.properties.iter().map(|(&name, value)| (name, value))
    }
}

/// One successful change to Truth, in the order it occurred.
#[derive(Debug, PartialEq, Eq)]
pub struct JournalEntry {
    /// Object created, reparented or edited, or the root of a deleted subtree.
    pub object: ObjectId,
    /// Enough state to reconstruct both sides of the change.
    pub change: Change,
}

/// The before and after data for an authoring edit.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    /// Creation of an empty object at the given location.
    Created {
        /// Registered reflected type of the new object.
        type_key: TypeKey,
        /// Parent assigned at creation.
        parent: Option<ObjectId>,
    },
    /// Change to a parent relationship.
    Reparented {
        /// Parent before the edit.
        old: Option<ObjectId>,
        /// Parent after the edit.
        new: Option<ObjectId>,
    },
    /// Setting or clearing a property.
    Property {
        /// Name from the reflected schema.
        name: &'static str,
        /// Previous authored value.
        old: Option<PropertyValue>,
        /// New authored value.
        new: Option<PropertyValue>,
    },
    /// Deletion of an object and its descendants.
    Deleted {
        /// Complete previous objects, with the root first.
        old: Vec<(ObjectId, TruthObject)>,
    },
}

/// An edit or journal operation rejected before changing Truth.
#[derive(Debug, thiserror::Error)]
pub enum TruthError {
    /// The requested type has no reflected registration.
    #[error("authoring type {0} has no reflected registration")]
    UnregisteredType(&'static str),
    /// The registered type cannot be used as a scene object.
    #[error("authoring object type {0} must be a reflected struct")]
    UnsupportedType(&'static str),
    /// An object with the same ID already exists.
    #[error("authored object {0} already exists")]
    DuplicateObject(ObjectId),
    /// The requested object or parent does not exist.
    #[error("authored object {0} does not exist")]
    MissingObject(ObjectId),
    /// The proposed parent would create a cycle.
    #[error("parent {parent} would create a cycle for object {object}")]
    ParentCycle {
        /// Object being moved.
        object: ObjectId,
        /// Proposed parent.
        parent: ObjectId,
    },
    /// The object's reflected type has no such field.
    #[error("authored object {object} has no field {name:?}")]
    UnknownProperty {
        /// Object being edited.
        object: ObjectId,
        /// Requested field name.
        name: String,
    },
    /// A property operation used the wrong Rust type.
    #[error("property expects {expected}, got {actual}")]
    PropertyType {
        /// Field's reflected Rust type name.
        expected: &'static str,
        /// Rust type supplied by the caller.
        actual: &'static str,
    },
    /// The value could not be captured as an immutable snapshot.
    #[error("could not snapshot property: {0}")]
    Encode(#[from] ron::Error),
    /// A property snapshot could not be read as its registered type.
    #[error("could not read property: {0}")]
    Decode(#[from] ron::error::SpannedError),
    /// The requested journal length exceeds the current history.
    #[error("cannot rewind to {requested} edits from {current} edits")]
    InvalidRewind {
        /// Requested journal length.
        requested: usize,
        /// Current journal length.
        current: usize,
    },
}

/// In-memory authoring state. All changes to objects go through the journal.
#[derive(Debug)]
pub struct Truth<'registry> {
    registry: &'registry TypeRegistry,
    objects: BTreeMap<ObjectId, TruthObject>,
    journal: Vec<JournalEntry>,
}

impl<'registry> Truth<'registry> {
    /// Creates an empty scene using the supplied reflected type registrations.
    pub fn new(registry: &'registry TypeRegistry) -> Self {
        Self {
            registry,
            objects: BTreeMap::new(),
            journal: Vec::new(),
        }
    }

    /// Registry that resolves the type keys of this scene.
    pub fn registry(&self) -> &'registry TypeRegistry {
        self.registry
    }

    /// Looks up an object in the current scene.
    pub fn object(&self, id: ObjectId) -> Option<&TruthObject> {
        self.objects.get(&id)
    }

    /// Visits live objects in stable ID order.
    pub fn objects(&self) -> impl ExactSizeIterator<Item = (ObjectId, &TruthObject)> {
        self.objects.iter().map(|(&id, object)| (id, object))
    }

    /// Successful edits in application order.
    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// Creates an empty object with an explicit ID and an optional existing parent.
    pub fn create<T: Reflect>(
        &mut self,
        id: ObjectId,
        parent: Option<ObjectId>,
    ) -> Result<(), TruthError> {
        if self.objects.contains_key(&id) {
            return Err(TruthError::DuplicateObject(id));
        }
        if let Some(parent) = parent {
            self.require_object(parent)?;
        }
        let registration = self
            .registry
            .get::<T>()
            .filter(|r| r.schema().is_some())
            .ok_or(TruthError::UnregisteredType(type_name::<T>()))?;
        if !matches!(
            registration.schema().map(|s| &s.shape),
            Some(TypeShape::Struct(_))
        ) {
            return Err(TruthError::UnsupportedType(registration.name()));
        }
        let type_key = registration.key();
        self.objects.insert(
            id,
            TruthObject {
                type_key,
                parent,
                children: BTreeSet::new(),
                properties: BTreeMap::new(),
            },
        );
        self.attach(id, parent);
        self.record(id, Change::Created { type_key, parent });
        Ok(())
    }

    /// Changes a parent relationship, checking for missing objects and cycles.
    pub fn reparent(&mut self, id: ObjectId, parent: Option<ObjectId>) -> Result<(), TruthError> {
        let old = self.require_object(id)?.parent;
        if old != parent {
            self.validate_parent(id, parent)?;
            self.set_parent(id, parent);
            self.record(id, Change::Reparented { old, new: parent });
        }
        Ok(())
    }

    /// Sets a field using its registered Rust type.
    pub fn set_property<T: Serialize + DeserializeOwned + 'static>(
        &mut self,
        id: ObjectId,
        name: &str,
        value: &T,
    ) -> Result<(), TruthError> {
        let field = self.field(id, name)?;
        if field.type_id != TypeId::of::<T>() {
            return Err(TruthError::PropertyType {
                expected: field.type_name,
                actual: type_name::<T>(),
            });
        }
        let snapshot = PropertyValue {
            type_id: field.type_id,
            type_name: field.type_name,
            data: ron::to_string(value)?.into(),
        };
        self.edit_property(id, field.name, Some(snapshot));
        Ok(())
    }

    /// Clears the authored value of a field.
    pub fn clear_property(&mut self, id: ObjectId, name: &str) -> Result<(), TruthError> {
        let field = self.field(id, name)?;
        self.edit_property(id, field.name, None);
        Ok(())
    }

    /// Removes an object and its descendants as one journal entry.
    /// References from surviving objects keep their target IDs.
    pub fn delete(&mut self, id: ObjectId) -> Result<(), TruthError> {
        let parent = self.require_object(id)?.parent;
        self.detach(id, parent);
        let mut pending = vec![id];
        let mut old = Vec::new();
        while let Some(id) = pending.pop() {
            let object = self.objects.remove(&id).expect("subtree object exists");
            pending.extend(object.children.iter().rev().copied());
            old.push((id, object));
        }
        self.record(id, Change::Deleted { old });
        Ok(())
    }

    /// Reverses and discards entries after the given length of the current journal.
    pub fn rewind(&mut self, len: usize) -> Result<(), TruthError> {
        if len > self.journal.len() {
            return Err(TruthError::InvalidRewind {
                requested: len,
                current: self.journal.len(),
            });
        }
        while self.journal.len() > len {
            let JournalEntry { object, change } = self.journal.pop().expect("entry exists");
            match change {
                Change::Created { parent, .. } => {
                    self.detach(object, parent);
                    self.objects.remove(&object);
                }
                Change::Reparented { old, .. } => self.set_parent(object, old),
                Change::Property { name, old, .. } => {
                    self.replace_property(object, name, old);
                }
                Change::Deleted { old } => {
                    self.objects.extend(old);
                    self.attach(object, self.objects[&object].parent);
                }
            }
        }
        Ok(())
    }

    fn require_object(&self, id: ObjectId) -> Result<&TruthObject, TruthError> {
        self.object(id).ok_or(TruthError::MissingObject(id))
    }

    fn field(&self, id: ObjectId, name: &str) -> Result<&'registry Field, TruthError> {
        let object = self.require_object(id)?;
        let shape = &self
            .registry
            .get_by_key(object.type_key)
            .and_then(|r| r.schema())
            .expect("registered object schema")
            .shape;
        let TypeShape::Struct(fields) = shape else {
            unreachable!("authoring objects have struct schemas");
        };
        fields
            .iter()
            .find(|field| field.name == name)
            .ok_or_else(|| TruthError::UnknownProperty {
                object: id,
                name: name.to_owned(),
            })
    }

    fn validate_parent(&self, id: ObjectId, parent: Option<ObjectId>) -> Result<(), TruthError> {
        let Some(parent) = parent else {
            return Ok(());
        };
        let mut current = Some(parent);
        while let Some(ancestor) = current {
            if ancestor == id {
                return Err(TruthError::ParentCycle { object: id, parent });
            }
            current = self.require_object(ancestor)?.parent;
        }
        Ok(())
    }

    fn set_parent(&mut self, id: ObjectId, parent: Option<ObjectId>) {
        self.detach(id, self.objects[&id].parent);
        self.attach(id, parent);
        self.objects.get_mut(&id).expect("object exists").parent = parent;
    }

    fn attach(&mut self, id: ObjectId, parent: Option<ObjectId>) {
        if let Some(parent) = parent {
            self.objects
                .get_mut(&parent)
                .expect("parent exists")
                .children
                .insert(id);
        }
    }

    fn detach(&mut self, id: ObjectId, parent: Option<ObjectId>) {
        if let Some(parent) = parent {
            self.objects
                .get_mut(&parent)
                .expect("parent exists")
                .children
                .remove(&id);
        }
    }

    fn edit_property(&mut self, id: ObjectId, name: &'static str, new: Option<PropertyValue>) {
        if self.objects[&id].property(name) != new.as_ref() {
            let old = self.replace_property(id, name, new.clone());
            self.record(id, Change::Property { name, old, new });
        }
    }

    fn replace_property(
        &mut self,
        id: ObjectId,
        name: &'static str,
        value: Option<PropertyValue>,
    ) -> Option<PropertyValue> {
        let properties = &mut self.objects.get_mut(&id).expect("object exists").properties;
        match value {
            Some(value) => properties.insert(name, value),
            None => properties.remove(name),
        }
    }

    fn record(&mut self, object: ObjectId, change: Change) {
        self.journal.push(JournalEntry { object, change });
    }
}
