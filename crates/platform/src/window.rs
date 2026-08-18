use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

pub fn run_empty_window() -> Result<(), winit::error::EventLoopError> {
    EventLoop::new()?.run_app(&mut EmptyApp::default())
}

#[derive(Default)]
struct EmptyApp {
    window: Option<Window>,
}

impl ApplicationHandler for EmptyApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = Window::default_attributes().with_title("rgine - bootstrap");
            self.window = Some(
                event_loop
                    .create_window(attrs)
                    .expect("window creation failed"),
            );
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
        }
    }
}
