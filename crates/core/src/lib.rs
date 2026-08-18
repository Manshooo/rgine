#![forbid(unsafe_code)]
//! Foundational runtime services.

pub mod logging;
pub mod time;

pub use glam;

pub fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
