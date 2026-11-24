use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

use super::{Pass, Resource, ResourceId, ResourceKind};

/// The inclusive interval of uses in compiled pass order.
/// Exports count as a use immediately after the last pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLifetime {
    /// The position of the first retained read, write, or export.
    pub first_use: usize,
    /// The position of the last retained read, write, or export.
    pub last_use: usize,
}

/// An opaque physical allocation within one compiled graph.
/// Multiple transients can share it when their lifetimes do not overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransientAllocationId {
    graph: u64,
    index: usize,
}

/// The backend objects allocated for the transients of one graph.
pub struct TransientAllocations<T> {
    graph: u64,
    objects: Vec<T>,
}

impl<T> TransientAllocations<T> {
    /// Resolves an allocation belonging to this graph.
    pub fn get(&self, allocation: TransientAllocationId) -> Option<&T> {
        if allocation.graph != self.graph {
            return None;
        }
        self.objects.get(allocation.index)
    }
}

pub(super) struct AllocationPlan<D> {
    graph: u64,
    lifetimes: Vec<Option<ResourceLifetime>>,
    slots: Vec<Option<usize>>,
    descriptors: Vec<D>,
}

impl<D> AllocationPlan<D> {
    pub(super) fn new<P>(
        graph: u64,
        resources: &[Resource<D>],
        passes: &[Pass<P>],
        order: &[usize],
        exports: &[ResourceId],
    ) -> Self
    where
        D: Clone + Eq + Hash,
    {
        let mut lifetimes = vec![None; resources.len()];
        for (position, &index) in order.iter().enumerate() {
            let pass = &passes[index];
            for resource in pass.reads.iter().chain(&pass.writes) {
                record_use(&mut lifetimes[resource.index], position);
            }
        }
        for resource in exports {
            record_use(&mut lifetimes[resource.index], order.len());
        }

        let mut transients = Vec::new();
        for (index, (resource, lifetime)) in resources.iter().zip(&lifetimes).enumerate() {
            if let (ResourceKind::Transient(Some(descriptor)), Some(lifetime)) =
                (&resource.kind, lifetime)
            {
                transients.push((index, descriptor, *lifetime));
            }
        }
        transients.sort_unstable_by_key(|&(index, _, lifetime)| (lifetime.first_use, index));

        let mut slots = vec![None; resources.len()];
        let mut descriptors = Vec::new();
        let mut by_descriptor = HashMap::<&D, BinaryHeap<Reverse<(usize, usize)>>>::new();
        for (index, descriptor, lifetime) in transients {
            let matching = by_descriptor.entry(descriptor).or_default();
            let slot = match matching.peek() {
                Some(&Reverse((last_use, slot))) if last_use < lifetime.first_use => {
                    matching.pop();
                    slot
                }
                _ => {
                    let slot = descriptors.len();
                    descriptors.push(descriptor.clone());
                    slot
                }
            };
            matching.push(Reverse((lifetime.last_use, slot)));
            slots[index] = Some(slot);
        }

        Self {
            graph,
            lifetimes,
            slots,
            descriptors,
        }
    }

    pub(super) fn lifetime(&self, resource: ResourceId) -> Option<ResourceLifetime> {
        self.lifetimes[self.resource_index(resource)?]
    }

    pub(super) fn allocation(&self, resource: ResourceId) -> Option<TransientAllocationId> {
        let index = self.slots[self.resource_index(resource)?]?;
        Some(TransientAllocationId {
            graph: self.graph,
            index,
        })
    }

    fn resource_index(&self, resource: ResourceId) -> Option<usize> {
        (resource.graph == self.graph).then_some(resource.index)
    }

    pub(super) fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub(super) fn allocate<T, E>(
        &self,
        allocate: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TransientAllocations<T>, E> {
        let objects = self
            .descriptors
            .iter()
            .map(allocate)
            .collect::<Result<_, _>>()?;
        Ok(TransientAllocations {
            graph: self.graph,
            objects,
        })
    }
}

fn record_use(lifetime: &mut Option<ResourceLifetime>, position: usize) {
    lifetime
        .get_or_insert(ResourceLifetime {
            first_use: position,
            last_use: position,
        })
        .last_use = position;
}
