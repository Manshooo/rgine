#![forbid(unsafe_code)]
//! Authored scene objects and their change journal (ADR 0004).

mod truth;

pub use truth::{Change, JournalEntry, ObjectId, PropertyValue, Truth, TruthError, TruthObject};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
