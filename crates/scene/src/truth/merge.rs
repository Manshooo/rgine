use std::collections::{BTreeMap, BTreeSet};

use ron::value::RawValue;

use super::{
    ObjectId, SceneError,
    persistence::{SCENE_VERSION, SavedObject, SceneFile, read_scene, write_scene},
};

type SavedNode = SavedObject<Box<RawValue>>;

/// A source snapshot could not be merged into a valid scene.
#[derive(Debug, thiserror::Error)]
pub enum SceneMergeError {
    /// One of the input snapshots could not be read.
    #[error("invalid {side} scene: {source}")]
    Input {
        /// Name of the snapshot that failed validation.
        side: &'static str,
        /// Format or hierarchy error in the snapshot.
        source: Box<SceneError>,
    },
    /// Concurrent edits cannot both be applied to a node or field.
    #[error("conflicting changes to object {object}, {field}")]
    Conflict {
        /// Authored object UUID.
        object: ObjectId,
        /// Field or object metadata affected by the conflict.
        field: String,
    },
    /// The combined parent edits would leave an invalid hierarchy.
    #[error("merged hierarchy conflicts: {0}")]
    Hierarchy(#[source] Box<SceneError>),
    /// The merged document could not be encoded.
    #[error("could not write merged RON: {0}")]
    Encode(#[from] ron::Error),
}

/// Merges three authored RON snapshots by node UUID and field name.
///
/// An empty base represents a file added on both sides. Property RON keeps its
/// syntax and stays opaque to the driver, so no reflected registry is needed.
/// The output is written only after the complete merged hierarchy validates.
pub fn merge_scene_ron(base: &str, ours: &str, theirs: &str) -> Result<String, SceneMergeError> {
    let base = if base.is_empty() {
        BTreeMap::new()
    } else {
        read_input("base", base)?
    };
    let ours = read_input("current", ours)?;
    let theirs = read_input("incoming", theirs)?;
    let objects = merge_map(&base, &ours, &theirs, |&object, base, ours, theirs| {
        merge_node(object, base, ours, theirs)
    })?;
    let scene = SceneFile {
        schema_version: SCENE_VERSION,
        objects,
    };
    scene
        .children()
        .map_err(|error| SceneMergeError::Hierarchy(Box::new(error)))?;
    Ok(write_scene(&scene)?)
}

fn read_input(
    side: &'static str,
    text: &str,
) -> Result<BTreeMap<ObjectId, SavedNode>, SceneMergeError> {
    let invalid = |source| SceneMergeError::Input {
        side,
        source: Box::new(source),
    };
    let (_, mut scene) = read_scene(text).map_err(invalid)?;
    scene.children().map_err(invalid)?;
    scene.trim_properties();
    Ok(scene.objects)
}

fn merge_node(
    object: ObjectId,
    base: Option<&SavedNode>,
    ours: Option<&SavedNode>,
    theirs: Option<&SavedNode>,
) -> Result<Option<SavedNode>, SceneMergeError> {
    if let Some(chosen) = choose(base, ours, theirs) {
        return Ok(chosen.cloned());
    }
    let conflict = |field| SceneMergeError::Conflict { object, field };
    let (Some(base), Some(ours), Some(theirs)) = (base, ours, theirs) else {
        return Err(conflict("object existence or creation".into()));
    };
    // A type or version change can alter the meaning of all fields.
    if ours.object_type != base.object_type || theirs.object_type != base.object_type {
        return Err(conflict("type declaration".into()));
    }
    let parent =
        choose(base.parent, ours.parent, theirs.parent).ok_or_else(|| conflict("parent".into()))?;
    let properties = merge_map(
        &base.properties,
        &ours.properties,
        &theirs.properties,
        |name, base, ours, theirs| {
            choose(base, ours, theirs)
                .map(|value| value.cloned())
                .ok_or_else(|| conflict(format!("property {name:?}")))
        },
    )?;
    Ok(Some(SavedNode {
        object_type: base.object_type.clone(),
        parent,
        properties,
    }))
}

fn choose<T: PartialEq>(base: T, ours: T, theirs: T) -> Option<T> {
    if ours == theirs || theirs == base {
        Some(ours)
    } else if ours == base {
        Some(theirs)
    } else {
        None
    }
}

fn merge_map<K: Ord + Clone, V>(
    base: &BTreeMap<K, V>,
    ours: &BTreeMap<K, V>,
    theirs: &BTreeMap<K, V>,
    mut merge: impl FnMut(&K, Option<&V>, Option<&V>, Option<&V>) -> Result<Option<V>, SceneMergeError>,
) -> Result<BTreeMap<K, V>, SceneMergeError> {
    let keys: BTreeSet<_> = base
        .keys()
        .chain(ours.keys())
        .chain(theirs.keys())
        .collect();
    let mut merged = BTreeMap::new();
    for key in keys {
        if let Some(value) = merge(key, base.get(key), ours.get(key), theirs.get(key))? {
            merged.insert(key.clone(), value);
        }
    }
    Ok(merged)
}
