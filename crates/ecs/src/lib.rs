#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! The runtime entity-component-system: entities, storage, queries, scheduling.
//!
//! The data model is [ADR 0010]. [`World`] stores table components in dense
//! archetype columns and sparse components in separate sets.
//!
//! [ADR 0010]: https://github.com/Manshooo/rgine/blob/master/docs/adr/0010-ecs-data-model.md

extern crate self as engine_ecs;

mod archetype;
mod commands;
mod component;
mod entity;
mod query;
mod relation;
mod schedule;
mod storage;
mod tick;
mod world;

pub use commands::{Commands, SpawnCommands};
pub use component::{
    Component, ComponentHook, ComponentLifecycle, ComponentMetadata, RequiredComponent, StorageKind,
};
pub use engine_ecs_macros::Component;
pub use entity::Entity;
pub use query::{Added, Changed, ComponentRef, Query, QueryData, QueryFilter, With, Without};
pub use relation::{ChildOf, Relation, RelationError};
pub use schedule::{Schedule, StageId};
pub use tick::Tick;
pub use world::World;
