//! Screen-space game UI. Authored layouts can be inspected without a renderer.

#![forbid(unsafe_code)]

#[cfg(feature = "canvas")]
pub mod canvas;
pub mod geometry;
#[cfg(feature = "input")]
pub mod input;
mod layout;
mod style;
pub mod text;

pub use layout::{LayoutError, Widget, WidgetKind, WidgetTree};
pub use style::{
    AlignItems, ComputedStyle, Dimension, Edges, Flow, JustifyContent, LineHeight, StyleError,
    StyleSheet, StyledWidget, TextAlign, Typography, WidgetState,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
