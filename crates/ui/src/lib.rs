//! Screen-space game UI. Authored layouts can be inspected without a renderer.

#![forbid(unsafe_code)]

mod layout;

pub use layout::{LayoutError, Widget, WidgetKind, WidgetTree};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
