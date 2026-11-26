//! Screen-space game UI. Authored layouts can be inspected without a renderer.

#![forbid(unsafe_code)]

mod layout;
mod style;

pub use layout::{LayoutError, Widget, WidgetKind, WidgetTree};
pub use style::{
    AlignItems, ComputedStyle, Dimension, Edges, Flow, JustifyContent, LineHeight, StyleError,
    StyleSheet, StyledWidget, TextAlign, Typography, WidgetState,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
