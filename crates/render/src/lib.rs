#![forbid(unsafe_code)]

//! GPU frame rendering. Native surfaces and wgpu objects stay behind this API.

pub mod canvas;
mod drawable;
pub mod graph;
pub mod profile;
pub mod resources;
pub mod scene;
mod shaders;

pub use drawable::{BenchmarkGraphics, DrawOutcome, Drawable, RenderError};
#[cfg(feature = "shader-reload")]
pub use shaders::SceneShaderSources;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
