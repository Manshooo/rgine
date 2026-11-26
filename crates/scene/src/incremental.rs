use std::{
    collections::{BTreeMap, BTreeSet},
    ptr,
    sync::Mutex,
};

use engine_core::TypeRegistry;
use engine_ecs::{ChildOf, Commands, Component, Entity, World};

use crate::{
    CompileError, ObjectId, SceneEntities, SceneReferences, Truth, TruthObject,
    compiler::scene_indices,
};

/// A scene update rejected before applying component and hierarchy changes.
#[derive(Debug, thiserror::Error)]
pub enum SceneUpdateError<L, S> {
    /// The snapshot uses another registry, whose type keys address different registrations.
    #[error("scene update uses a different authoring type registry")]
    RegistryMismatch,
    /// A runtime edit invalidated an entity or hierarchy edge owned by this scene.
    #[error("runtime entity or parent for authored object {0} changed outside the scene compiler")]
    WorldChanged(ObjectId),
    /// The new scene could not be lowered.
    #[error(transparent)]
    Compile(#[from] CompileError<L>),
    /// The game could not prepare components for an object.
    #[error("could not prepare runtime data for authored object {object}: {source}")]
    Prepare {
        /// Authored ID of the node that failed.
        object: ObjectId,
        /// Error returned by the component preparation callback.
        source: S,
    },
}

/// Queues changes to the authored components of one live scene entity.
pub struct SceneEntityCommands<'a> {
    commands: &'a mut Commands,
    entity: Entity,
}

impl SceneEntityCommands<'_> {
    /// Queues insertion or replacement of a component.
    pub fn insert<T: Component>(&mut self, component: T) {
        self.commands.insert(self.entity, component);
    }

    /// Queues the removal of a component omitted by the updated payload.
    pub fn remove<T: Component>(&mut self) {
        self.commands.remove::<T>(self.entity);
    }
}

struct LiveNode {
    authored: TruthObject,
    entity: Entity,
    dependencies: BTreeSet<ObjectId>,
}

struct PlannedNode<'a, T> {
    id: ObjectId,
    authored: &'a TruthObject,
    data: T,
    dependencies: BTreeSet<ObjectId>,
}

/// A development world that can reconcile successive resolved Truth snapshots.
///
/// The compiler owns the identity and parent edge of each authored entity.
/// Runtime systems can keep independent components on those entities.
pub struct IncrementalScene<'registry> {
    world: World,
    compiler: SceneCompiler<'registry>,
}

impl<'registry> IncrementalScene<'registry> {
    /// Creates an empty development world using the given authoring registry.
    pub fn new(registry: &'registry TypeRegistry) -> Self {
        Self {
            world: World::new(),
            compiler: SceneCompiler::new(registry),
        }
    }

    /// Borrows the active world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Borrows the active world for simulation.
    /// Scene-owned entities and their parent edges are maintained by the compiler.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Finds the live entity for an authored object.
    pub fn entity(&self, object: ObjectId) -> Option<Entity> {
        self.compiler.entity(&self.world, object)
    }

    /// Finds the authored ID for a live entity owned by this scene.
    pub fn object(&self, entity: Entity) -> Option<ObjectId> {
        self.compiler.object(&self.world, entity)
    }

    /// Applies a snapshot to this world using the incremental compiler.
    pub fn update<T, L, S>(
        &mut self,
        truth: &Truth<'registry>,
        lower: impl FnMut(ObjectId, &TruthObject, &SceneReferences<'_>) -> Result<T, L>,
        prepare: impl FnMut(&mut SceneEntityCommands<'_>, &T, &SceneEntities<'_>) -> Result<(), S>,
    ) -> Result<(), SceneUpdateError<L, S>> {
        self.compiler.update(&mut self.world, truth, lower, prepare)
    }
}

/// Reconciles Truth snapshots in a caller-owned development world.
///
/// Use one compiler with the same world for the lifetime of its scene. The
/// compiler manages that scene's entity lifetimes and parent edges.
pub struct SceneCompiler<'registry> {
    registry: &'registry TypeRegistry,
    nodes: BTreeMap<ObjectId, LiveNode>,
    owners: BTreeMap<Entity, ObjectId>,
}

impl<'registry> SceneCompiler<'registry> {
    /// Creates an empty compiler using the given authoring registry.
    pub fn new(registry: &'registry TypeRegistry) -> Self {
        Self {
            registry,
            nodes: BTreeMap::new(),
            owners: BTreeMap::new(),
        }
    }

    /// Finds the live entity for an authored object in the paired world.
    pub fn entity(&self, world: &World, object: ObjectId) -> Option<Entity> {
        let entity = self.nodes.get(&object)?.entity;
        world.contains(entity).then_some(entity)
    }

    /// Finds the authored ID for a live entity owned by this scene.
    pub fn object(&self, world: &World, entity: Entity) -> Option<ObjectId> {
        self.owners
            .get(&entity)
            .copied()
            .filter(|_| world.contains(entity))
    }

    /// Applies a snapshot while retaining every surviving object's entity handle.
    ///
    /// Use the same deterministic lowering on each update. The prepare callback
    /// queues the complete set of authored component edits for each affected node.
    /// Resolve node indices to entities within that callback. Lowering or preparation
    /// errors leave the previous scene active.
    pub fn update<T, L, S>(
        &mut self,
        world: &mut World,
        truth: &Truth<'registry>,
        mut lower: impl FnMut(ObjectId, &TruthObject, &SceneReferences<'_>) -> Result<T, L>,
        mut prepare: impl FnMut(&mut SceneEntityCommands<'_>, &T, &SceneEntities<'_>) -> Result<(), S>,
    ) -> Result<(), SceneUpdateError<L, S>> {
        if !ptr::eq(self.registry, truth.registry()) {
            return Err(SceneUpdateError::RegistryMismatch);
        }
        if let Some(object) = self.changed_runtime_object(world) {
            return Err(SceneUpdateError::WorldChanged(object));
        }
        let indices = scene_indices(truth)?;
        let affected = self.affected(truth);
        let removed: Vec<_> = self
            .nodes
            .keys()
            .copied()
            .filter(|&id| truth.object(id).is_none())
            .collect();
        if affected.is_empty() && removed.is_empty() {
            return Ok(());
        }

        let mut plan = Vec::with_capacity(affected.len());
        for id in affected {
            let authored = truth
                .object(id)
                .expect("affected node belongs to new snapshot");
            let looked_up = Mutex::default();
            let references = SceneReferences::tracking(&indices, &looked_up);
            let data = lower(id, authored, &references)
                .map_err(|source| CompileError::Object { object: id, source })?;
            plan.push(PlannedNode {
                id,
                authored,
                data,
                dependencies: looked_up.into_inner().expect("reference tracking lock"),
            });
        }

        let mut spawned = Vec::new();
        let entities: Vec<_> = indices
            .keys()
            .map(|id| {
                self.nodes.get(id).map_or_else(
                    || {
                        let entity = world.spawn_empty();
                        spawned.push(entity);
                        entity
                    },
                    |node| node.entity,
                )
            })
            .collect();
        let references = SceneEntities::new(&entities);
        let entity_for = |object| {
            references
                .get(indices[&object])
                .expect("node was allocated")
        };
        let mut commands = Commands::default();
        for node in &plan {
            let mut edit = SceneEntityCommands {
                commands: &mut commands,
                entity: entity_for(node.id),
            };
            if let Err(source) = prepare(&mut edit, &node.data, &references) {
                for entity in spawned {
                    world.despawn(entity);
                }
                return Err(SceneUpdateError::Prepare {
                    object: node.id,
                    source,
                });
            }
        }

        world.advance_tick();
        // Detach all changed edges before attaching the new ones. Both snapshots
        // can be acyclic even when a one-edge-at-a-time transition would form a cycle.
        for node in &plan {
            if let Some(old_parent) = self
                .old_parent(node.id)
                .filter(|&old| Some(old) != node.authored.parent())
            {
                world.remove_relation::<ChildOf>(
                    entity_for(node.id),
                    self.nodes[&old_parent].entity,
                );
            }
        }
        for node in &plan {
            if let Some(parent) = node
                .authored
                .parent()
                .filter(|&parent| Some(parent) != self.old_parent(node.id))
            {
                world
                    .add_relation::<ChildOf>(entity_for(node.id), entity_for(parent))
                    .expect("validated snapshots form a scene hierarchy");
            }
        }
        for id in removed {
            let entity = self
                .nodes
                .remove(&id)
                .expect("removed node belongs to previous snapshot")
                .entity;
            self.owners.remove(&entity);
            world.despawn(entity);
        }
        commands.apply(world);
        for node in plan {
            let entity = entity_for(node.id);
            self.owners.insert(entity, node.id);
            self.nodes.insert(
                node.id,
                LiveNode {
                    authored: node.authored.clone(),
                    entity,
                    dependencies: node.dependencies,
                },
            );
        }
        Ok(())
    }

    fn changed_runtime_object(&self, world: &World) -> Option<ObjectId> {
        self.nodes.iter().find_map(|(&id, node)| {
            let expected_parent = node
                .authored
                .parent()
                .map(|parent| self.nodes[&parent].entity);
            (!world.contains(node.entity)
                || world.relation_targets::<ChildOf>(node.entity).next() != expected_parent)
                .then_some(id)
        })
    }

    fn old_parent(&self, object: ObjectId) -> Option<ObjectId> {
        self.nodes
            .get(&object)
            .and_then(|node| node.authored.parent())
    }

    fn affected(&self, truth: &Truth<'registry>) -> BTreeSet<ObjectId> {
        let mut subtrees = Vec::new();
        let mut parents = Vec::new();
        for (id, authored) in truth.objects() {
            let Some(old) = self.nodes.get(&id) else {
                subtrees.push(id);
                continue;
            };
            let references_changed = old
                .dependencies
                .iter()
                .any(|&target| self.nodes.contains_key(&target) != truth.object(target).is_some());
            if old.authored.type_key() != authored.type_key()
                || old.authored.parent() != authored.parent()
                || !old.authored.properties().eq(authored.properties())
                || references_changed
            {
                subtrees.push(id);
            } else if !old.authored.children().eq(authored.children()) {
                parents.push(id);
            }
        }
        let mut affected = BTreeSet::new();
        while let Some(id) = subtrees.pop() {
            if affected.insert(id) {
                subtrees.extend(
                    truth
                        .object(id)
                        .expect("subtree belongs to new snapshot")
                        .children(),
                );
            }
        }
        affected.extend(parents);
        affected
    }
}
