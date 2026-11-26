#![forbid(unsafe_code)]
//! Compiled scenes and optional Truth authoring, persistence and compilation.

mod compiled;
#[cfg(feature = "authoring")]
mod compiler;
#[cfg(feature = "authoring")]
mod truth;

pub use compiled::{CompiledScene, SceneEntities, SceneLoadError, SceneNodeId};
#[cfg(all(feature = "authoring", debug_assertions))]
pub use compiler::AuthoringTrace;
#[cfg(feature = "authoring")]
pub use compiler::{CompileError, SceneCompilation, SceneReferences};
#[cfg(feature = "authoring")]
pub use truth::{
    Change, JournalEntry, ObjectId, PrefabError, PrefabInstance, PropertyValue, SceneError,
    SceneTypeName, Truth, TruthError, TruthObject, register_prefabs,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
