//! Backend-independent resource dependencies and ordered frame execution.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GRAPH_ID: AtomicU64 = AtomicU64::new(0);

/// A logical resource declared by one render graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId {
    graph: u64,
    index: usize,
}

struct Resource {
    name: String,
    imported: bool,
}

/// A pass declaration and the data used to execute it.
pub struct Pass<P> {
    name: String,
    reads: Vec<ResourceId>,
    writes: Vec<ResourceId>,
    data: P,
}

impl<P> Pass<P> {
    /// The label used in diagnostics and GPU markers.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The resource versions consumed by the pass.
    pub fn reads(&self) -> &[ResourceId] {
        &self.reads
    }

    /// The resource versions produced by the pass.
    pub fn writes(&self) -> &[ResourceId] {
        &self.writes
    }

    /// The caller's execution data.
    pub fn data(&self) -> &P {
        &self.data
    }
}

/// A graph under construction. Each logical resource has at most one writer.
pub struct RenderGraph<P> {
    id: u64,
    resources: Vec<Resource>,
    passes: Vec<Pass<P>>,
    exports: Vec<ResourceId>,
}

impl<P> Default for RenderGraph<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> RenderGraph<P> {
    /// Creates an empty graph with its own resource handles.
    pub fn new() -> Self {
        Self {
            id: NEXT_GRAPH_ID.fetch_add(1, Ordering::Relaxed),
            resources: Vec::new(),
            passes: Vec::new(),
            exports: Vec::new(),
        }
    }

    /// Declares a resource supplied by the caller, such as a drawable texture.
    pub fn import(&mut self, name: impl Into<String>) -> ResourceId {
        self.resource(name.into(), true)
    }

    /// Declares a resource whose contents a pass must produce before use.
    pub fn transient(&mut self, name: impl Into<String>) -> ResourceId {
        self.resource(name.into(), false)
    }

    fn resource(&mut self, name: String, imported: bool) -> ResourceId {
        let id = ResourceId {
            graph: self.id,
            index: self.resources.len(),
        };
        self.resources.push(Resource { name, imported });
        id
    }

    /// Declares a pass. The order of its dependencies is resolved at compile time.
    pub fn add_pass(
        &mut self,
        name: impl Into<String>,
        reads: &[ResourceId],
        writes: &[ResourceId],
        data: P,
    ) {
        self.passes.push(Pass {
            name: name.into(),
            reads: reads.to_vec(),
            writes: writes.to_vec(),
            data,
        });
    }

    /// Declares a graph result consumed by the caller, for example during presentation.
    pub fn export(&mut self, resource: ResourceId) {
        self.exports.push(resource);
    }

    /// Validates resource access and orders passes by their dependencies.
    ///
    /// Among passes that are ready, the one declared first executes first.
    pub fn compile(self) -> Result<CompiledGraph<P>, GraphError> {
        let producers = self.producers()?;
        let mut remaining = vec![0; self.passes.len()];
        let mut dependents = vec![Vec::new(); self.passes.len()];
        for (index, pass) in self.passes.iter().enumerate() {
            for &resource in &pass.reads {
                if let Some(producer) = self.producer(resource, &pass.name, &producers)? {
                    remaining[index] += 1;
                    dependents[producer].push(index);
                }
            }
        }
        for &resource in &self.exports {
            self.producer(resource, "graph output", &producers)?;
        }

        let mut ready: BinaryHeap<_> = remaining
            .iter()
            .enumerate()
            .filter(|&(_, count)| *count == 0)
            .map(|(index, _)| Reverse(index))
            .collect();
        let mut order = Vec::with_capacity(self.passes.len());
        while let Some(Reverse(index)) = ready.pop() {
            order.push(index);
            for &dependent in &dependents[index] {
                remaining[dependent] -= 1;
                if remaining[dependent] == 0 {
                    ready.push(Reverse(dependent));
                }
            }
        }
        if let Some(unresolved) = remaining.iter().position(|&count| count != 0) {
            return Err(self.cycle(unresolved, &producers, &remaining));
        }

        Ok(CompiledGraph {
            passes: self.passes,
            order,
            exports: self.exports,
        })
    }

    fn checked_resource(&self, resource: ResourceId, consumer: &str) -> Result<usize, GraphError> {
        if resource.graph != self.id {
            return Err(GraphError::ForeignResource {
                consumer: consumer.to_owned(),
            });
        }
        Ok(resource.index)
    }

    fn producers(&self) -> Result<Vec<Option<usize>>, GraphError> {
        let mut producers: Vec<Option<usize>> = vec![None; self.resources.len()];
        for (index, pass) in self.passes.iter().enumerate() {
            for &resource in &pass.writes {
                let resource = self.checked_resource(resource, &pass.name)?;
                if let Some(first) = producers[resource].replace(index) {
                    if first != index {
                        return Err(GraphError::MultipleProducers {
                            resource: self.resources[resource].name.clone(),
                            first: self.passes[first].name.clone(),
                            second: pass.name.clone(),
                        });
                    }
                }
            }
        }
        Ok(producers)
    }

    fn producer(
        &self,
        resource: ResourceId,
        consumer: &str,
        producers: &[Option<usize>],
    ) -> Result<Option<usize>, GraphError> {
        let index = self.checked_resource(resource, consumer)?;
        let producer = producers[index];
        if producer.is_none() && !self.resources[index].imported {
            return Err(GraphError::MissingProducer {
                resource: self.resources[index].name.clone(),
                consumer: consumer.to_owned(),
            });
        }
        Ok(producer)
    }

    fn cycle(
        &self,
        mut cursor: usize,
        producers: &[Option<usize>],
        remaining: &[usize],
    ) -> GraphError {
        // Each unresolved pass still needs an unresolved producer. Following
        // those edges leads back to a pass on the path, even if the starting
        // pass only depends on the cycle.
        let mut position = vec![None; self.passes.len()];
        let mut path = Vec::new();
        while position[cursor].is_none() {
            position[cursor] = Some(path.len());
            path.push(cursor);
            cursor = self.passes[cursor]
                .reads
                .iter()
                .filter_map(|resource| producers[resource.index])
                .find(|&producer| remaining[producer] != 0)
                .expect("an unresolved pass has an unresolved producer");
        }
        let start = position[cursor].expect("the repeated pass is on the path");
        let passes = path[start..]
            .iter()
            .copied()
            .chain(std::iter::once(cursor))
            .map(|index| self.passes[index].name.clone())
            .collect();
        GraphError::Cycle { passes }
    }
}

/// A validated graph ready to execute with the caller's resource bindings.
pub struct CompiledGraph<P> {
    passes: Vec<Pass<P>>,
    order: Vec<usize>,
    exports: Vec<ResourceId>,
}

impl<P> CompiledGraph<P> {
    /// Declared outputs, in export order.
    pub fn exports(&self) -> &[ResourceId] {
        &self.exports
    }

    /// Invokes the backend once per pass in compiled order, stopping on error.
    pub fn execute<E>(&self, mut run: impl FnMut(&Pass<P>) -> Result<(), E>) -> Result<(), E> {
        for &index in &self.order {
            run(&self.passes[index])?;
        }
        Ok(())
    }
}

/// Why a render graph could not be compiled.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphError {
    /// An access uses a handle from a different graph.
    ForeignResource {
        /// The pass or output using the handle.
        consumer: String,
    },
    /// A resource was read or exported without a source.
    MissingProducer {
        /// The resource without a source.
        resource: String,
        /// The pass or output that needs the resource.
        consumer: String,
    },
    /// More than one pass writes the same logical resource.
    MultipleProducers {
        /// The resource with conflicting writers.
        resource: String,
        /// The first writer in declaration order.
        first: String,
        /// The second writer in declaration order.
        second: String,
    },
    /// Passes depend on each other in a cycle.
    Cycle {
        /// The dependency path, with the starting pass repeated at the end.
        passes: Vec<String>,
    },
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignResource { consumer } => {
                write!(
                    formatter,
                    "{consumer:?} uses a resource from a different graph"
                )
            }
            Self::MissingProducer { resource, consumer } => {
                write!(
                    formatter,
                    "{consumer:?} needs {resource:?}, which has no producer"
                )
            }
            Self::MultipleProducers {
                resource,
                first,
                second,
            } => write!(
                formatter,
                "both {first:?} and {second:?} write {resource:?}"
            ),
            Self::Cycle { passes } => {
                write!(
                    formatter,
                    "render graph dependency cycle: {}",
                    passes.join(" -> ")
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}
