//! Entity rows grouped by their table component types.

use std::collections::HashMap;
use std::sync::Arc;

use engine_core::reflection::TypeKey;

use crate::Entity;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Location {
    pub(crate) archetype: usize,
    pub(crate) row: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Edge {
    Add,
    Remove,
}

impl Edge {
    fn reverse(self) -> Self {
        match self {
            Self::Add => Self::Remove,
            Self::Remove => Self::Add,
        }
    }
}

#[derive(Debug, Default)]
struct Archetype {
    signature: Arc<[TypeKey]>,
    entities: Vec<Entity>,
    edges: HashMap<(TypeKey, Edge), usize>,
}

#[derive(Debug, Default)]
pub(crate) struct Archetypes {
    tables: Vec<Archetype>,
    by_signature: HashMap<Arc<[TypeKey]>, usize>,
    locations: Vec<Location>,
}

impl Archetypes {
    pub(crate) fn spawn(&mut self, entity: Entity) {
        if self.tables.is_empty() {
            let empty = Archetype::default();
            self.by_signature.insert(empty.signature.clone(), 0);
            self.tables.push(empty);
        }

        if entity.index() as usize == self.locations.len() {
            self.locations.push(Location::default());
        }
        self.attach(entity, 0);
    }

    /// Only call with an entity whose generation was checked by `World`.
    pub(crate) fn location(&self, entity: Entity) -> Location {
        self.locations[entity.index() as usize]
    }

    pub(crate) fn signature(&self, archetype: usize) -> &[TypeKey] {
        &self.tables[archetype].signature
    }

    pub(crate) fn transition(&mut self, from: usize, component: TypeKey, edge: Edge) -> usize {
        let source = &self.tables[from];
        if let Some(&to) = source.edges.get(&(component, edge)) {
            return to;
        }

        let mut signature = source.signature.to_vec();
        match edge {
            Edge::Add => {
                let index = signature
                    .binary_search(&component)
                    .expect_err("new table component");
                signature.insert(index, component);
            }
            Edge::Remove => {
                let index = signature
                    .binary_search(&component)
                    .expect("existing table component");
                signature.remove(index);
            }
        }
        let to = if let Some(&to) = self.by_signature.get(signature.as_slice()) {
            to
        } else {
            let to = self.tables.len();
            let signature = Arc::<[TypeKey]>::from(signature);
            self.by_signature.insert(signature.clone(), to);
            self.tables.push(Archetype {
                signature,
                ..Archetype::default()
            });
            to
        };

        self.tables[from].edges.insert((component, edge), to);
        self.tables[to]
            .edges
            .insert((component, edge.reverse()), from);
        to
    }

    pub(crate) fn move_entity(&mut self, from: Location, to: usize) {
        let entity = self.detach(from);
        self.attach(entity, to);
    }

    pub(crate) fn detach(&mut self, location: Location) -> Entity {
        let rows = &mut self.tables[location.archetype].entities;
        let entity = rows.swap_remove(location.row);
        if let Some(swapped) = rows.get(location.row) {
            self.locations[swapped.index() as usize] = location;
        }
        entity
    }

    fn attach(&mut self, entity: Entity, archetype: usize) {
        let rows = &mut self.tables[archetype].entities;
        self.locations[entity.index() as usize] = Location {
            archetype,
            row: rows.len(),
        };
        rows.push(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entities;
    use engine_core::TypeRegistry;

    #[test]
    fn edges_reuse_the_same_archetype_in_both_directions() {
        let mut registry = TypeRegistry::default();
        let first = registry.register::<u32>().key();
        let second = registry.register::<bool>().key();
        let mut tables = Archetypes::default();
        tables.spawn(Entities::default().alloc());

        let with_first = tables.transition(0, first, Edge::Add);
        let with_both = tables.transition(with_first, second, Edge::Add);
        let with_second = tables.transition(0, second, Edge::Add);
        assert_eq!(tables.transition(with_second, first, Edge::Add), with_both);
        assert_eq!(
            tables.transition(with_both, second, Edge::Remove),
            with_first
        );
        assert_eq!(tables.transition(with_first, second, Edge::Add), with_both);
        assert_eq!(
            tables.transition(with_both, first, Edge::Remove),
            with_second
        );
        assert_eq!(tables.signature(with_both), &[first, second]);
        assert_eq!(tables.tables.len(), 4);
        assert_eq!(
            tables.tables[with_first].edges[&(second, Edge::Add)],
            with_both
        );
        assert_eq!(
            tables.tables[with_both].edges[&(second, Edge::Remove)],
            with_first
        );
    }
}
