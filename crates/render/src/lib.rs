#![forbid(unsafe_code)]

//! GPU frame rendering. Native surfaces and wgpu objects stay behind this API.

mod drawable;
pub mod graph;
pub mod resources;
pub mod scene;
mod shaders;

pub use drawable::{DrawOutcome, Drawable, RenderError};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
