use std::fmt;

use engine_platform::{Commands, PlatformError, SurfaceSource, WindowEvent, WindowId};
use engine_render::{DrawOutcome, Drawable};

/// A failure from a rendered application.
#[derive(Debug)]
#[non_exhaustive]
pub enum RenderError {
    /// The platform could not run the application.
    Platform(PlatformError),
    /// The renderer could not initialize or present a frame.
    Graphics(engine_render::RenderError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => error.fmt(formatter),
            Self::Graphics(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Graphics(error) => Some(error),
        }
    }
}

enum Surface {
    Pending(SurfaceSource),
    Ready(Box<Drawable>),
}

pub(crate) struct Frames {
    surface: Option<Surface>,
    color: [f64; 4],
    size: [u32; 2],
    occluded: bool,
}

impl Frames {
    pub(crate) fn new(color: [f64; 4]) -> Self {
        Self {
            surface: None,
            color,
            size: [0, 0],
            occluded: false,
        }
    }

    pub(crate) fn attach(&mut self, source: SurfaceSource, size: [u32; 2]) {
        self.surface = Some(Surface::Pending(source));
        self.size = size;
        self.occluded = false;
    }

    pub(crate) fn detach(&mut self) {
        self.surface = None;
    }

    pub(crate) fn window_event(
        &mut self,
        event: WindowEvent,
        window: WindowId,
        commands: &mut Commands,
    ) -> bool {
        match event {
            WindowEvent::Resized { width, height } => {
                self.size = [width, height];
                if !self.size.contains(&0) {
                    commands.request_redraw(&[window]);
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    commands.request_redraw(&[window]);
                }
            }
            WindowEvent::RedrawRequested => return true,
            _ => {}
        }
        false
    }

    pub(crate) fn draw(
        &mut self,
        windows: &[WindowId],
        commands: &mut Commands,
    ) -> Result<(), RenderError> {
        if self.occluded || self.size.contains(&0) {
            return Ok(());
        }
        if let Some(Surface::Pending(source)) = self
            .surface
            .take_if(|surface| matches!(surface, Surface::Pending(_)))
        {
            let drawable = Drawable::new(source, self.size).map_err(RenderError::Graphics)?;
            self.surface = Some(Surface::Ready(Box::new(drawable)));
        }
        if let Some(Surface::Ready(drawable)) = &mut self.surface {
            drawable.resize(self.size);
            if drawable.draw(self.color).map_err(RenderError::Graphics)? == DrawOutcome::Retry {
                commands.request_redraw(windows);
            }
        }
        Ok(())
    }
}
