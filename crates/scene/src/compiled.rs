use engine_ecs::{ChildOf, Entity, RelationError, World};
use serde::{Deserialize, Serialize};

const COMPILED_SCENE_VERSION: u32 = 1;

/// An index into one compiled scene, assigned by the authoring compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SceneNodeId(pub(crate) u32);

impl std::fmt::Display for SceneNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Resolves compiled references inside the current instantiation.
pub struct SceneEntities<'a>(&'a [Entity]);

impl SceneEntities<'_> {
    /// Finds the entity handle allocated for this node index.
    pub fn get(&self, node: SceneNodeId) -> Option<Entity> {
        self.0.get(node.0 as usize).copied()
    }
}

/// Why a compiled scene could not be instantiated.
#[derive(Debug, thiserror::Error)]
pub enum SceneLoadError<E> {
    /// The compiled scene envelope has no compatible loader.
    #[error("unsupported compiled scene version {0}; expected {COMPILED_SCENE_VERSION}")]
    UnsupportedVersion(u32),
    /// A parent index is outside the scene.
    #[error("scene node {node} has missing parent {parent}")]
    MissingParent {
        /// Child node index.
        node: SceneNodeId,
        /// Invalid parent index.
        parent: SceneNodeId,
    },
    /// The ECS rejected a hierarchy edge.
    #[error("scene node {node} has an invalid parent: {source}")]
    InvalidHierarchy {
        /// Child node index.
        node: SceneNodeId,
        /// Why the edge was rejected.
        source: RelationError,
    },
    /// The game could not install runtime data for a node.
    #[error("could not load runtime data for scene node {node}: {source}")]
    Node {
        /// Node index.
        node: SceneNodeId,
        /// Error returned by the game's loader.
        source: E,
    },
    /// The node count exceeds the capacity of scene indices.
    #[error("compiled scene exceeds the capacity of 32-bit node indices")]
    TooManyNodes,
}

/// Versioned runtime data that can be serialized without authoring types or ECS handles.
/// `T` is the game's spawn data, which can contain [`SceneNodeId`] references.
#[derive(Debug, Serialize, Deserialize)]
pub struct CompiledScene<T> {
    schema_version: u32,
    nodes: Vec<CompiledNode<T>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CompiledNode<T> {
    pub(crate) parent: Option<SceneNodeId>,
    pub(crate) data: T,
}

impl<T> CompiledScene<T> {
    #[cfg(feature = "authoring")]
    pub(crate) fn new(nodes: Vec<CompiledNode<T>>) -> Self {
        Self {
            schema_version: COMPILED_SCENE_VERSION,
            nodes,
        }
    }

    /// Builds a fresh world. The callback installs components after all nodes and parent edges exist.
    /// It is called in node index order and can resolve forward references using `SceneEntities`.
    pub fn instantiate<E>(
        &self,
        load: impl FnMut(&mut World, Entity, &T, &SceneEntities<'_>) -> Result<(), E>,
    ) -> Result<World, SceneLoadError<E>> {
        self.instantiate_with_entities(load).map(|(world, _)| world)
    }

    pub(crate) fn instantiate_with_entities<E>(
        &self,
        mut load: impl FnMut(&mut World, Entity, &T, &SceneEntities<'_>) -> Result<(), E>,
    ) -> Result<(World, Vec<Entity>), SceneLoadError<E>> {
        if self.schema_version != COMPILED_SCENE_VERSION {
            return Err(SceneLoadError::UnsupportedVersion(self.schema_version));
        }
        if u32::try_from(self.nodes.len()).is_err() {
            return Err(SceneLoadError::TooManyNodes);
        }
        let mut world = World::new();
        let entities: Vec<_> = self.nodes.iter().map(|_| world.spawn_empty()).collect();
        let references = SceneEntities(&entities);
        for (index, (node, &entity)) in self.nodes.iter().zip(&entities).enumerate() {
            if let Some(parent) = node.parent {
                let node = SceneNodeId(index as u32);
                let parent_entity = references
                    .get(parent)
                    .ok_or(SceneLoadError::MissingParent { node, parent })?;
                world
                    .add_relation::<ChildOf>(entity, parent_entity)
                    .map_err(|source| SceneLoadError::InvalidHierarchy { node, source })?;
            }
        }
        for (index, (node, &entity)) in self.nodes.iter().zip(&entities).enumerate() {
            load(&mut world, entity, &node.data, &references).map_err(|source| {
                SceneLoadError::Node {
                    node: SceneNodeId(index as u32),
                    source,
                }
            })?;
        }
        Ok((world, entities))
    }
}
