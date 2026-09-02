//! Window handles and descriptions.

/// An opaque handle to a window owned by the platform layer.
///
/// Callers hold the handle; the platform owns the window itself. Handles are
/// allocated by [`Commands::create_windows`](crate::Commands::create_windows)
/// and stay valid until the matching [`WindowEvent::Destroyed`](crate::WindowEvent::Destroyed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
