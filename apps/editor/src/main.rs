#![forbid(unsafe_code)]

use std::{error::Error, path::PathBuf};

use engine_platform::{
    Commands, Event, PlatformApp, SurfaceEvent, WindowDesc, WindowEvent, WindowId,
};
use engine_render::{DrawOutcome, Drawable, canvas::Canvas};

mod document;
mod hierarchy;
mod view;

use document::Document;
use hierarchy::Hierarchy;
use view::View;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let initial_path = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("usage: engine-editor [level.scene.ron]".into());
    }
    engine_core::init_logging();
    let registry = document::registry();
    let mut hierarchy = Hierarchy::new(Document::new(&registry));
    if let Some(path) = initial_path
        && let Err(error) = hierarchy.document.open(&path)
    {
        hierarchy.notice = error;
    }
    let mut editor = Editor {
        hierarchy,
        view: View::new()?,
        canvas: Canvas::default(),
        window: None,
        surface: None,
        error: None,
    };
    engine_platform::run(&mut editor)?;
    editor.error.map_or(Ok(()), Err)
}

struct Editor<'registry> {
    hierarchy: Hierarchy<'registry>,
    view: View,
    canvas: Canvas,
    window: Option<WindowId>,
    surface: Option<Drawable>,
    error: Option<Box<dyn Error>>,
}

impl Editor<'_> {
    fn draw(&mut self) -> Result<DrawOutcome, Box<dyn Error>> {
        let Some(surface) = self.surface.as_mut() else {
            return Ok(DrawOutcome::Idle);
        };
        let Ok(viewport) = surface.canvas_viewport() else {
            return Ok(DrawOutcome::Idle);
        };
        self.canvas.clear();
        self.view.draw(
            &self.hierarchy,
            viewport,
            surface.resources(),
            &mut self.canvas,
        )?;
        Ok(surface.draw_with_canvas([0.022, 0.029, 0.042, 1.0], &self.canvas)?)
    }
}

impl PlatformApp for &mut Editor<'_> {
    fn surface_event(&mut self, event: SurfaceEvent) {
        match event {
            SurfaceEvent::Available(window, source, size) if Some(window) == self.window => {
                match Drawable::new(source, size) {
                    Ok(surface) => self.surface = Some(surface),
                    Err(error) => self.error = Some(Box::new(error)),
                }
            }
            SurfaceEvent::Invalidating(window) if Some(window) == self.window => {
                self.surface = None
            }
            _ => {}
        }
    }

    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        if self.error.is_some() {
            commands.exit();
            return;
        }
        if self.window.is_none() && events.contains(&Event::Resumed) {
            let mut ids = Vec::new();
            commands.create_windows(
                &[WindowDesc::new("rgine Editor").with_size(960, 630)],
                &mut ids,
            );
            self.window = ids.first().copied();
            commands.request_redraw(&ids);
        }
        let Some(window) = self.window else { return };
        let text_input = self.hierarchy.accepts_text();
        let mut dirty = false;
        let mut draw = false;
        for event in events {
            match event {
                Event::Window {
                    window: target,
                    event,
                } if *target == window => match event {
                    WindowEvent::CloseRequested => {
                        self.hierarchy.close();
                        dirty = true;
                    }
                    WindowEvent::Destroyed => commands.exit(),
                    WindowEvent::Resized { width, height } => {
                        if let Some(surface) = &mut self.surface {
                            surface.resize([*width, *height]);
                        }
                        dirty = true;
                    }
                    WindowEvent::RedrawRequested => draw = true,
                    WindowEvent::ScaleFactorChanged | WindowEvent::Occluded(false) => dirty = true,
                    _ => {}
                },
                Event::Input(event) if event.window == window => {
                    if let Some(viewport) = self
                        .surface
                        .as_ref()
                        .and_then(|surface| surface.canvas_viewport().ok())
                    {
                        dirty |= self.hierarchy.input(
                            event.input,
                            viewport.display_scale(),
                            viewport.logical_size(),
                        );
                    }
                }
                Event::TextInput {
                    window: target,
                    text,
                } if *target == window => {
                    dirty |= self.hierarchy.text(text);
                }
                _ => {}
            }
        }
        if self.hierarchy.exit {
            commands.exit();
            return;
        }
        if text_input != self.hierarchy.accepts_text() {
            commands.set_text_input(window, self.hierarchy.accepts_text());
        }
        if draw {
            match self.draw() {
                Ok(DrawOutcome::Retry) => commands.request_redraw(&[window]),
                Ok(_) => {}
                Err(error) => {
                    self.error = Some(error);
                    commands.exit();
                }
            }
        } else if dirty {
            commands.request_redraw(&[window]);
        }
    }
}
