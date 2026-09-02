//! Commands recorded by the application and applied by the platform.
//!
//! Commands travel in one direction only: the application records them, the
//! platform applies them at the end of the loop iteration. Nothing here returns
//! a backend value, so none of it is a synchronization point.

use crate::{WindowDesc, WindowId};

/// A single queued request to the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Create a window and bind it to the given handle.
    CreateWindow {
        /// Handle reserved for the new window.
        window: WindowId,
        /// How the window should be created.
        desc: WindowDesc,
    },
    /// Destroy a window; its handle becomes stale afterwards.
    CloseWindow(WindowId),
    /// Ask a window to redraw.
    RequestRedraw(WindowId),
    /// Leave the event loop and return from [`run`](crate::run).
    Exit,
}

/// The queue an application records platform requests into.
///
/// A fresh queue is handed to every
/// [`PlatformApp::update`](crate::PlatformApp::update) call and drained by the
/// platform once that call returns.
#[derive(Debug, Default)]
pub struct Commands {
    queue: Vec<Command>,
    next_window: u64,
}

impl Commands {
    /// Reserves a handle per entry of `descs`, appends the handles to `out` in
    /// the same order, and queues the creation of each window.
    ///
    /// The handles are allocated by this queue rather than by the backend, so
    /// they are usable immediately and this call does not synchronize with the
    /// platform.
    pub fn create_windows(&mut self, descs: &[WindowDesc], out: &mut Vec<WindowId>) {
        out.reserve(descs.len());
        self.queue.reserve(descs.len());
        for desc in descs {
            let window = WindowId::from_raw(self.next_window);
            self.next_window += 1;
            out.push(window);
            self.queue.push(Command::CreateWindow {
                window,
                desc: desc.clone(),
            });
        }
    }

    /// Queues the destruction of every window in `windows`.
    pub fn close_windows(&mut self, windows: &[WindowId]) {
        self.queue
            .extend(windows.iter().copied().map(Command::CloseWindow));
    }

    /// Queues a redraw request for every window in `windows`.
    pub fn request_redraw(&mut self, windows: &[WindowId]) {
        self.queue
            .extend(windows.iter().copied().map(Command::RequestRedraw));
    }

    /// Queues an exit from the event loop.
    pub fn exit(&mut self) {
        self.queue.push(Command::Exit);
    }

    /// Removes and returns everything recorded so far, in the order it was
    /// recorded.
    ///
    /// The platform calls this once per loop iteration. It is public so that
    /// application-side loop policy can be tested without an event loop.
    pub fn drain(&mut self) -> Vec<Command> {
        core::mem::take(&mut self.queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_windows_allocates_one_distinct_handle_per_desc() {
        let mut commands = Commands::default();
        let descs = [WindowDesc::new("a"), WindowDesc::new("b")];
        let mut handles = Vec::new();

        commands.create_windows(&descs, &mut handles);

        assert_eq!(handles.len(), 2);
        assert_ne!(handles[0], handles[1]);
        assert_eq!(
            commands.drain(),
            vec![
                Command::CreateWindow {
                    window: handles[0],
                    desc: descs[0].clone(),
                },
                Command::CreateWindow {
                    window: handles[1],
                    desc: descs[1].clone(),
                },
            ]
        );
    }

    #[test]
    fn handles_stay_unique_across_drains() {
        let mut commands = Commands::default();
        let mut first = Vec::new();
        let mut second = Vec::new();

        commands.create_windows(&[WindowDesc::default()], &mut first);
        let _ = commands.drain();
        commands.create_windows(&[WindowDesc::default()], &mut second);

        assert_ne!(first[0], second[0]);
    }

    #[test]
    fn drain_preserves_record_order_and_empties_the_queue() {
        let mut commands = Commands::default();
        let mut handles = Vec::new();
        commands.create_windows(&[WindowDesc::default()], &mut handles);
        commands.request_redraw(&handles);
        commands.close_windows(&handles);
        commands.exit();

        let drained = commands.drain();

        assert!(matches!(drained[0], Command::CreateWindow { .. }));
        assert_eq!(drained[1], Command::RequestRedraw(handles[0]));
        assert_eq!(drained[2], Command::CloseWindow(handles[0]));
        assert_eq!(drained[3], Command::Exit);
        assert!(commands.drain().is_empty());
    }
}
