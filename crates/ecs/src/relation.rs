//! Typed edges and their reverse index.

use std::any::TypeId;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::Entity;

/// Identifies a kind of directed, acyclic entity relationship.
pub trait Relation: 'static {
    /// Whether a source can have only one target of this kind.
    /// Adding a different target replaces the old edge.
    const EXCLUSIVE: bool = false;
}

/// Connects a child (source) to its sole parent (target).
pub enum ChildOf {}

impl Relation for ChildOf {
    const EXCLUSIVE: bool = true;
}

/// Why an entity relationship could not be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationError {
    /// One endpoint is no longer alive.
    StaleEntity(Entity),
    /// The proposed edge would form a cycle of this relation kind.
    Cycle,
}

impl fmt::Display for RelationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleEntity(entity) => write!(f, "entity {entity} is not alive"),
            Self::Cycle => f.write_str("relationship would form a cycle"),
        }
    }
}

impl std::error::Error for RelationError {}

type Adjacency = HashMap<Entity, HashMap<TypeId, BTreeSet<Entity>>>;

#[derive(Debug, Default)]
pub(crate) struct Relations {
    outgoing: Adjacency,
    incoming: Adjacency,
}

impl Relations {
    pub(crate) fn insert<R: Relation>(
        &mut self,
        source: Entity,
        target: Entity,
    ) -> Result<bool, RelationError> {
        let kind = TypeId::of::<R>();
        if self
            .outgoing
            .get(&source)
            .and_then(|kinds| kinds.get(&kind))
            .is_some_and(|targets| targets.contains(&target))
        {
            return Ok(false);
        }
        if self.reaches(kind, target, source) {
            return Err(RelationError::Cycle);
        }
        if R::EXCLUSIVE {
            let previous = self.targets::<R>(source).next();
            if let Some(previous) = previous {
                self.remove::<R>(source, previous);
            }
        }
        connect(&mut self.outgoing, kind, source, target);
        connect(&mut self.incoming, kind, target, source);
        Ok(true)
    }

    pub(crate) fn remove<R: Relation>(&mut self, source: Entity, target: Entity) -> bool {
        let kind = TypeId::of::<R>();
        let removed = disconnect(&mut self.outgoing, kind, source, target);
        if removed {
            disconnect(&mut self.incoming, kind, target, source);
        }
        removed
    }

    pub(crate) fn targets<R: Relation>(&self, source: Entity) -> impl Iterator<Item = Entity> + '_ {
        neighbors(&self.outgoing, TypeId::of::<R>(), source)
    }

    pub(crate) fn sources<R: Relation>(&self, target: Entity) -> impl Iterator<Item = Entity> + '_ {
        neighbors(&self.incoming, TypeId::of::<R>(), target)
    }

    pub(crate) fn despawn(&mut self, entity: Entity) {
        disconnect_all(&mut self.outgoing, &mut self.incoming, entity);
        disconnect_all(&mut self.incoming, &mut self.outgoing, entity);
    }

    fn reaches(&self, kind: TypeId, start: Entity, goal: Entity) -> bool {
        if start == goal {
            return true;
        }
        let mut pending: Vec<_> = neighbors(&self.outgoing, kind, start).collect();
        let mut visited = HashSet::new();
        while let Some(entity) = pending.pop() {
            if entity == goal {
                return true;
            }
            if visited.insert(entity) {
                pending.extend(neighbors(&self.outgoing, kind, entity));
            }
        }
        false
    }
}

fn neighbors(index: &Adjacency, kind: TypeId, entity: Entity) -> impl Iterator<Item = Entity> + '_ {
    index
        .get(&entity)
        .and_then(|kinds| kinds.get(&kind))
        .into_iter()
        .flatten()
        .copied()
}

fn connect(index: &mut Adjacency, kind: TypeId, from: Entity, to: Entity) {
    index
        .entry(from)
        .or_default()
        .entry(kind)
        .or_default()
        .insert(to);
}

fn disconnect(index: &mut Adjacency, kind: TypeId, from: Entity, to: Entity) -> bool {
    let Some(kinds) = index.get_mut(&from) else {
        return false;
    };
    let Some(entities) = kinds.get_mut(&kind) else {
        return false;
    };
    let removed = entities.remove(&to);
    if entities.is_empty() {
        kinds.remove(&kind);
    }
    if kinds.is_empty() {
        index.remove(&from);
    }
    removed
}

fn disconnect_all(index: &mut Adjacency, opposite: &mut Adjacency, entity: Entity) {
    for (kind, neighbors) in index.remove(&entity).into_iter().flatten() {
        for neighbor in neighbors {
            disconnect(opposite, kind, neighbor, entity);
        }
    }
}
