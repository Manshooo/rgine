//! Window handles and descriptions.

use std::{fmt, sync::Arc};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use serde::{Deserialize, Serialize};

pub(crate) trait SurfaceHandles: HasDisplayHandle + HasWindowHandle + Send + Sync {
    fn pre_present_notify(&self);
}

/// Owned native handle source for a renderer's surface.
///
/// The renderer must drop all copies and surfaces using them on invalidation.
#[derive(Clone)]
pub struct SurfaceSource(Arc<dyn SurfaceHandles>);

impl fmt::Debug for SurfaceSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurfaceSource")
            .finish_non_exhaustive()
    }
}

impl SurfaceSource {
    pub(crate) fn new(window: Arc<dyn SurfaceHandles>) -> Self {
        Self(window)
    }

    /// Notifies the platform just before a frame is presented.
    pub fn pre_present_notify(&self) {
        self.0.pre_present_notify();
    }
}

impl HasDisplayHandle for SurfaceSource {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}

impl HasWindowHandle for SurfaceSource {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

/// A drawable lifetime transition delivered synchronously by the platform.
#[non_exhaustive]
pub enum SurfaceEvent {
    /// The window now has a native source and initial size in physical pixels.
    Available(WindowId, SurfaceSource, [u32; 2]),
    /// Drop this window's surface and native source before the callback returns.
    Invalidating(WindowId),
}

/// An opaque handle to a window owned by the platform layer.
///
/// Callers hold the handle; the platform owns the window itself. Handles are
/// allocated by [`Commands::create_windows`](crate::Commands::create_windows)
/// and stay valid until the matching [`WindowEvent::Destroyed`](crate::WindowEvent::Destroyed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(u64);

impl WindowId {
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// How a window should be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDesc {
    /// Title shown by the window manager, where one exists.
    pub title: String,
    /// Requested width in logical pixels; the platform may not honour it.
    pub width: u32,
    /// Requested height in logical pixels; the platform may not honour it.
    pub height: u32,
}

impl Default for WindowDesc {
    fn default() -> Self {
        Self {
            title: "rgine".to_string(),
            width: 1280,
            height: 720,
        }
    }
}

impl WindowDesc {
    /// A description with the given title and the default size.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Sets the requested size in logical pixels.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }
}
