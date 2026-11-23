//! Clear-color and native window lifetime spike for issue #37.
//!
//! cargo run --locked -p engine-render --example surface_spike -- --smoke

use engine_platform::{
    Commands, Event, PlatformApp, SurfaceEvent, WindowDesc, WindowEvent, WindowId,
};

use engine_render::{DrawOutcome, Drawable};

const COLOR: [f64; 4] = [0.026, 0.22, 0.40, 1.0];

type Error = Box<dyn std::error::Error>;

#[derive(Default)]
struct Spike {
    window: Option<WindowId>,
    drawable: Option<Drawable>,
    failure: Option<Error>,
    size: [u32; 2],
    smoke: bool,
    first_frame_presented: bool,
}

impl Spike {
    fn update_window(
        &mut self,
        event: WindowEvent,
        window: WindowId,
        commands: &mut Commands,
    ) -> Result<(), Error> {
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            commands.exit();
        }
        let Some(drawable) = &mut self.drawable else {
            return Ok(());
        };
        match event {
            WindowEvent::Resized { width, height } => {
                self.size = [width, height];
                drawable.resize(self.size);
                commands.request_redraw(&[window]);
            }
            WindowEvent::RedrawRequested => {
                match drawable.draw(COLOR)? {
                    DrawOutcome::Presented => {}
                    DrawOutcome::Retry => {
                        commands.request_redraw(&[window]);
                        return Ok(());
                    }
                    DrawOutcome::Idle => return Ok(()),
                }
                if !self.first_frame_presented {
                    println!("Presented clear color");
                    self.first_frame_presented = true;
                    if self.smoke {
                        drawable.resize([0, 0]);
                        if !matches!(drawable.draw(COLOR)?, DrawOutcome::Idle) {
                            return Err("zero-sized drawable unexpectedly presented".into());
                        }
                        println!("Skipped zero-sized drawable");
                        drawable.resize(self.size);
                        drawable.recreate()?;
                        commands.request_redraw(&[window]);
                    }
                } else if self.smoke {
                    println!("Presented after recreating surface");
                    commands.exit();
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl PlatformApp for &mut Spike {
    fn surface_event(&mut self, event: SurfaceEvent) {
        match event {
            SurfaceEvent::Available(window, source, size) if self.window == Some(window) => {
                self.size = size;
                match Drawable::new(source, size) {
                    Ok(drawable) => self.drawable = Some(drawable),
                    Err(error) => self.failure = Some(error.into()),
                }
            }
            SurfaceEvent::Invalidating(window) if self.window == Some(window) => {
                self.drawable = None;
            }
            _ => {}
        }
    }

    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        if self.failure.is_some() {
            commands.exit();
            return;
        }
        for event in events {
            match *event {
                Event::Resumed if self.window.is_none() => {
                    let mut windows = Vec::new();
                    commands.create_windows(
                        &[WindowDesc::new("rgine surface spike").with_size(640, 480)],
                        &mut windows,
                    );
                    self.window = windows.pop();
                    if let Some(window) = self.window {
                        commands.request_redraw(&[window]);
                    }
                }
                Event::Suspended => {
                    if let Some(window) = self.window.take() {
                        commands.close_windows(&[window]);
                    }
                }
                Event::Window { window, event } if self.window == Some(window) => {
                    if let Err(error) = self.update_window(event, window, commands) {
                        self.failure = Some(error);
                        commands.exit();
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

fn main() -> Result<(), Error> {
    let mut spike = Spike {
        smoke: std::env::args().any(|arg| arg == "--smoke"),
        ..Default::default()
    };
    engine_platform::run(&mut spike)?;
    spike.failure.map_or(Ok(()), Err)
}
