//! Clear-color and native window lifetime spike for issue #37.
//!
//! cargo run --locked -p engine-render --example surface_spike -- --smoke

use engine_platform::{
    Commands, Event, PlatformApp, SurfaceEvent, SurfaceSource, WindowDesc, WindowEvent, WindowId,
};

type Error = Box<dyn std::error::Error>;

enum DrawOutcome {
    Presented,
    Retry,
    Idle,
}

struct Drawable {
    surface: Option<wgpu::Surface<'static>>,
    source: SurfaceSource,
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: [u32; 2],
}

impl Drawable {
    async fn new(source: SurfaceSource, size: [u32; 2]) -> Result<Self, Error> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(source.clone())),
        );
        let surface = instance.create_surface(source.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let info = adapter.get_info();
        println!("Adapter: {} ({:?})", info.name, info.backend);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let config = surface
            .get_default_config(&adapter, size[0].max(1), size[1].max(1))
            .ok_or("adapter cannot configure the drawable")?;
        let mut drawable = Self {
            surface: Some(surface),
            source,
            instance,
            device,
            queue,
            config,
            size: [0, 0],
        };
        drawable.resize(size);
        Ok(drawable)
    }

    fn resize(&mut self, size: [u32; 2]) {
        if self.size != size {
            self.size = size;
            self.configure();
        }
    }

    fn configure(&mut self) {
        let [width, height] = self.size;
        if width == 0 || height == 0 {
            return;
        }
        if let Some(surface) = &self.surface {
            self.config.width = width;
            self.config.height = height;
            surface.configure(&self.device, &self.config);
            println!("Configured {width}x{height}");
        }
    }

    fn recreate(&mut self) -> Result<(), Error> {
        drop(self.surface.take());
        self.surface = Some(self.instance.create_surface(self.source.clone())?);
        println!("Recreated surface");
        self.configure();
        Ok(())
    }

    fn draw(&mut self) -> Result<DrawOutcome, Error> {
        use wgpu::CurrentSurfaceTexture;

        if self.size.contains(&0) {
            return Ok(DrawOutcome::Idle);
        }
        let Some(surface) = &self.surface else {
            return Ok(DrawOutcome::Idle);
        };
        let (frame, suboptimal) = match surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => (frame, false),
            CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            CurrentSurfaceTexture::Outdated => {
                self.configure();
                return Ok(DrawOutcome::Retry);
            }
            CurrentSurfaceTexture::Lost => {
                self.recreate()?;
                return Ok(DrawOutcome::Retry);
            }
            CurrentSurfaceTexture::Timeout => return Ok(DrawOutcome::Retry),
            CurrentSurfaceTexture::Occluded => return Ok(DrawOutcome::Idle),
            CurrentSurfaceTexture::Validation => {
                return Err("surface texture acquisition failed validation".into());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.026,
                        g: 0.22,
                        b: 0.40,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        if suboptimal {
            self.configure();
        }
        Ok(DrawOutcome::Presented)
    }
}

#[derive(Default)]
struct Spike {
    window: Option<WindowId>,
    drawable: Option<Drawable>,
    failure: Option<Error>,
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
                drawable.resize([width, height]);
                commands.request_redraw(&[window]);
            }
            WindowEvent::RedrawRequested => {
                match drawable.draw()? {
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
                        let size = drawable.size;
                        drawable.resize([0, 0]);
                        if !matches!(drawable.draw()?, DrawOutcome::Idle) {
                            return Err("zero-sized drawable unexpectedly presented".into());
                        }
                        println!("Skipped zero-sized drawable");
                        drawable.resize(size);
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
                match pollster::block_on(Drawable::new(source, size)) {
                    Ok(drawable) => self.drawable = Some(drawable),
                    Err(error) => self.failure = Some(error),
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
