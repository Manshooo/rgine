#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! The runtime entity-component-system: entities, storage, queries, scheduling.
//!
//! The data model is [ADR 0010]. Phase 1 builds it in the order the decisions
//! depend on each other, starting with entity identity: everything else in the
//! crate is indexed by [`Entity`], so the handle has to be settled before the
//! first component column exists.
//!
//! [ADR 0010]: https://github.com/Manshooo/rgine/blob/master/docs/adr/0010-ecs-data-model.md

mod entity;
mod world;

pub use entity::Entity;
pub use world::World;
