#![forbid(unsafe_code)]
//! Authored scene objects, versioned RON persistence and the change journal.

mod truth;

pub use truth::{
    Change, JournalEntry, ObjectId, PropertyValue, SceneError, SceneTypeName, Truth, TruthError,
    TruthObject,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
