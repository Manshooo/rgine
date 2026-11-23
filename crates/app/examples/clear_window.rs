//! Present a clear color through the regular application loop.
//!
//! cargo run --locked -p engine-app --example clear_window

use engine_app::{App, RenderError, platform::WindowDesc};

pub(crate) fn main() -> Result<(), RenderError> {
    engine_app::core::init_logging();
    App::new()
        .with_window(WindowDesc::new("rgine - clear window").with_size(640, 480))
        .run_rendered([0.026, 0.22, 0.4, 1.0])
}
