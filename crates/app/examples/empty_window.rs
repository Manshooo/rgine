//! The Phase 0 exit criterion, runnable: an empty window opened by [`App`].
//!
//! ```text
//! cargo run -p engine-app --example empty_window
//! ```
//!
//! Nothing is drawn - there is no renderer yet. What the example proves is that
//! the loop policy in `engine-app` drives a real platform event loop: the
//! window opens when the platform resumes, and closing it returns from `run`
//! instead of terminating the process.

use engine_app::{App, platform::PlatformError, platform::WindowDesc};

fn main() -> Result<(), PlatformError> {
    engine_app::core::init_logging();

    App::new()
        .with_window(WindowDesc::new("rgine - empty window").with_size(1280, 720))
        .run()
}
