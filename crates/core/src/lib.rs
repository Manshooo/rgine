#![forbid(unsafe_code)]
//! Foundational runtime services.

extern crate self as engine_core;

pub mod logging;
pub mod reflection;
pub mod time;

pub use engine_core_macros::Reflect;
pub use glam;
pub use reflection::{Reflect, TypeRegistry};

#[doc(hidden)]
pub mod __private {
    pub use crate::__reflect_editor as reflect_editor;
}

// Select the expansion in the owner crate, even when a downstream crate also
// defines an `editor-metadata` feature of its own.
#[doc(hidden)]
#[macro_export]
#[cfg(feature = "editor-metadata")]
macro_rules! __reflect_editor {
    ($value:expr, $editor:expr) => {{
        let mut value = $value;
        value.editor = $editor;
        value
    }};
}

#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "editor-metadata"))]
macro_rules! __reflect_editor {
    ($value:expr, $editor:expr) => {
        $value
    };
}

pub fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
