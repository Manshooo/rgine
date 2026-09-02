//! winit-backed event loop.

use std::collections::HashMap;

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent as BackendWindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId as BackendWindowId},
};

use crate::{Command, Commands, Event, PlatformApp, PlatformError, WindowEvent, WindowId};

pub(crate) fn run<A: PlatformApp>(app: A) -> Result<(), PlatformError> {
    let event_loop =
        EventLoop::new().map_err(|error| PlatformError::EventLoop(error.to_string()))?;
    // The contract is that the application is updated once per loop iteration,
    // which is also what a fixed timestep will need. Under the default `Wait`
    // the loop sleeps until the OS sends something, so updates would only
    // happen on input and a command recorded during the last update - `Exit`
    // included - would not be acted on until the next unrelated message
    // arrived. That is observable as a window that stays open after it is
    // closed.
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut driver = Driver::new(app);
    event_loop
        .run_app(&mut driver)
        .map_err(|error| PlatformError::EventLoop(error.to_string()))?;

    match driver.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Translates between the backend event loop and the [`PlatformApp`] contract.
struct Driver<A> {
    app: A,
    /// Events accumulated since the last `update`, flushed as one batch.
    events: Vec<Event>,
    /// Kept across iterations so window handles stay unique for the whole run.
    commands: Commands,
    windows: HashMap<WindowId, Window>,
    handles: HashMap<BackendWindowId, WindowId>,
    error: Option<PlatformError>,
}

impl<A: PlatformApp> Driver<A> {
    fn new(app: A) -> Self {
        Self {
            app,
            events: Vec::new(),
            commands: Commands::default(),
            windows: HashMap::new(),
            handles: HashMap::new(),
            error: None,
        }
    }

    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: WindowId,
        desc: &crate::WindowDesc,
    ) -> Result<(), PlatformError> {
        let attributes = Window::default_attributes()
            .with_title(desc.title.clone())
            .with_inner_size(LogicalSize::new(desc.width, desc.height));
        let created = event_loop
            .create_window(attributes)
            .map_err(|error| PlatformError::WindowCreation(error.to_string()))?;

        self.handles.insert(created.id(), window);
        self.windows.insert(window, created);
        Ok(())
    }

    fn apply(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        match command {
            Command::CreateWindow { window, desc } => {
                if let Err(error) = self.create_window(event_loop, window, &desc) {
                    self.fail(event_loop, error);
                }
            }
            Command::CloseWindow(window) => {
                if let Some(closed) = self.windows.remove(&window) {
                    self.handles.remove(&closed.id());
                    drop(closed);
                    self.events.push(Event::Window {
                        window,
                        event: WindowEvent::Destroyed,
                    });
                }
            }
            Command::RequestRedraw(window) => {
                if let Some(target) = self.windows.get(&window) {
                    target.request_redraw();
                }
            }
            Command::Exit => event_loop.exit(),
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: PlatformError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
        event_loop.exit();
    }
}

impl<A: PlatformApp> ApplicationHandler for Driver<A> {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        self.events.push(Event::Resumed);
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.events.push(Event::Suspended);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: BackendWindowId,
        event: BackendWindowEvent,
    ) {
        // A window closed by command is unmapped before the backend reports its
        // destruction, so events for it are dropped here instead of reaching the
        // application through a stale handle.
        let Some(&window) = self.handles.get(&window_id) else {
            return;
        };

        let event = match event {
            BackendWindowEvent::CloseRequested => WindowEvent::CloseRequested,
            BackendWindowEvent::RedrawRequested => WindowEvent::RedrawRequested,
            BackendWindowEvent::Resized(size) => WindowEvent::Resized {
                width: size.width,
                height: size.height,
            },
            BackendWindowEvent::Destroyed => {
                self.handles.remove(&window_id);
                self.windows.remove(&window);
                WindowEvent::Destroyed
            }
            _ => return,
        };

        self.events.push(Event::Window { window, event });
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.error.is_some() {
            return;
        }

        let events = core::mem::take(&mut self.events);
        self.app.update(&events, &mut self.commands);

        for command in self.commands.drain() {
            self.apply(event_loop, command);
            if self.error.is_some() {
                return;
            }
        }
    }
}
