#![forbid(unsafe_code)]

pub use engine_core as core;
pub use engine_ecs as ecs;
pub use engine_platform as platform;

pub trait Plugin {
    fn build(&self, app: &mut App);
}

#[derive(Default)]
pub struct App {
    plugins: Vec<Box<dyn Plugin>>,
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_plugin<P: Plugin + 'static>(mut self, plugin: P) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    pub fn run(self) -> Result<(), winit::error::EventLoopError> {
        engine_platform::window::run_empty_window()
    }
}
