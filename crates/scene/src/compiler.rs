use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

#[cfg(debug_assertions)]
use engine_ecs::{Entity, World};

use crate::compiled::CompiledNode;
use crate::{CompiledScene, ObjectId, SceneNodeId, Truth, TruthObject};
#[cfg(debug_assertions)]
use crate::{SceneEntities, SceneLoadError};

/// Looks up an authored reference in the complete scene being compiled.
pub struct SceneReferences<'a> {
    indices: &'a BTreeMap<ObjectId, SceneNodeId>,
    looked_up: Option<&'a Mutex<BTreeSet<ObjectId>>>,
}

impl<'a> SceneReferences<'a> {
    pub(crate) fn tracking(
        indices: &'a BTreeMap<ObjectId, SceneNodeId>,
        looked_up: &'a Mutex<BTreeSet<ObjectId>>,
    ) -> Self {
        Self {
            indices,
            looked_up: Some(looked_up),
        }
    }

    /// Resolves a scene-local object ID to its compiled node index.
    pub fn get(&self, object: ObjectId) -> Option<SceneNodeId> {
        if let Some(looked_up) = self.looked_up {
            looked_up
                .lock()
                .expect("reference tracking lock")
                .insert(object);
        }
        self.indices.get(&object).copied()
    }
}

/// Why Truth could not be compiled into runtime data.
#[derive(Debug, thiserror::Error)]
pub enum CompileError<E> {
    /// Prefab links must be resolved against their sources before cooking.
    #[error("resolve prefab instance {0} before compiling the scene")]
    UnresolvedPrefab(ObjectId),
    /// The node count exceeds the capacity of scene indices.
    #[error("authored scene exceeds the capacity of 32-bit node indices")]
    TooManyNodes,
    /// The game's compiler rejected an object.
    #[error("could not compile authored object {object}: {source}")]
    Object {
        /// The authored object being compiled.
        object: ObjectId,
        /// Error from the game's compiler callback.
        source: E,
    },
}

/// The result of compiling a complete scene.
pub struct SceneCompilation<T> {
    scene: CompiledScene<T>,
    #[cfg(debug_assertions)]
    objects: Vec<ObjectId>,
}

impl<T> SceneCompilation<T> {
    /// Takes the runtime data for serialization or loading.
    pub fn into_compiled(self) -> CompiledScene<T> {
        self.scene
    }

    /// Instantiates a development world and records the relationship between its entities and Truth.
    #[cfg(debug_assertions)]
    pub fn instantiate_traced<E>(
        &self,
        load: impl FnMut(&mut World, Entity, &T, &SceneEntities<'_>) -> Result<(), E>,
    ) -> Result<(World, AuthoringTrace), SceneLoadError<E>> {
        let (world, entities) = self.scene.instantiate_with_entities(load)?;
        let pairs = self.objects.iter().copied().zip(entities);
        let trace = AuthoringTrace {
            entities: pairs.clone().collect(),
            objects: pairs.map(|(object, entity)| (entity, object)).collect(),
        };
        Ok((world, trace))
    }
}

/// Development snapshot linking compiled entity handles to the authored IDs that produced them.
#[cfg(debug_assertions)]
pub struct AuthoringTrace {
    entities: BTreeMap<ObjectId, Entity>,
    objects: BTreeMap<Entity, ObjectId>,
}

#[cfg(debug_assertions)]
impl AuthoringTrace {
    /// Finds the entity that was instantiated for an authored object.
    pub fn entity(&self, object: ObjectId) -> Option<Entity> {
        self.entities.get(&object).copied()
    }

    /// Finds the authored object that produced this entity handle.
    pub fn object(&self, entity: Entity) -> Option<ObjectId> {
        self.objects.get(&entity).copied()
    }
}

impl Truth<'_> {
    /// Compiles a complete snapshot in ascending object ID order.
    /// The callback lowers reflected properties to deterministic, game-defined spawn data.
    /// Its reference lookup includes every object, including nodes compiled later in the pass.
    pub fn compile<T, E>(
        &self,
        mut compile_object: impl FnMut(ObjectId, &TruthObject, &SceneReferences<'_>) -> Result<T, E>,
    ) -> Result<SceneCompilation<T>, CompileError<E>> {
        let indices = scene_indices(self)?;
        let references = SceneReferences {
            indices: &indices,
            looked_up: None,
        };
        let mut nodes = Vec::with_capacity(indices.len());
        for (id, object) in self.objects() {
            let data = compile_object(id, object, &references)
                .map_err(|source| CompileError::Object { object: id, source })?;
            nodes.push(CompiledNode {
                parent: object.parent().map(|parent| indices[&parent]),
                data,
            });
        }
        Ok(SceneCompilation {
            scene: CompiledScene::new(nodes),
            #[cfg(debug_assertions)]
            objects: indices.into_keys().collect(),
        })
    }
}

pub(crate) fn scene_indices<E>(
    truth: &Truth<'_>,
) -> Result<BTreeMap<ObjectId, SceneNodeId>, CompileError<E>> {
    if let Some(object) = truth.first_prefab() {
        return Err(CompileError::UnresolvedPrefab(object));
    }
    let objects = truth.objects();
    if u32::try_from(objects.len()).is_err() {
        return Err(CompileError::TooManyNodes);
    }
    Ok(objects
        .enumerate()
        .map(|(index, (object, _))| (object, SceneNodeId(index as u32)))
        .collect())
}
