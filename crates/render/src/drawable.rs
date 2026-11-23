use std::fmt;

use engine_platform::SurfaceSource;
use wgpu::CurrentSurfaceTexture;

/// A GPU or surface failure reported without exposing backend types.
#[derive(Debug)]
pub struct RenderError(String);

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RenderError {}

/// The result of trying to present a frame.
#[derive(Debug, PartialEq, Eq)]
pub enum DrawOutcome {
    /// The frame was submitted and presented.
    Presented,
    /// Request another redraw after a temporary acquisition failure.
    Retry,
    /// Wait for the drawable to become visible or nonempty.
    Idle,
}

/// The GPU state for one live window surface.
///
/// Drop this when the platform invalidates its source.
pub struct Drawable {
    surface: Option<wgpu::Surface<'static>>,
    source: SurfaceSource,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: [u32; 2],
    needs_configure: bool,
}

impl Drawable {
    /// Initializes the GPU backend for a live surface. Call outside native
    /// lifecycle callbacks, since adapter and device creation can block.
    pub fn new(source: SurfaceSource, size: [u32; 2]) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(source.clone())),
        );
        let surface = create_surface(&instance, &source)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|error| RenderError(format!("could not select a graphics adapter: {error}")))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|error| {
                    RenderError(format!("could not open a graphics device: {error}"))
                })?;
        let info = adapter.get_info();
        tracing::info!(adapter = %info.name, backend = ?info.backend, "graphics device ready");
        Ok(Self {
            surface: Some(surface),
            source,
            instance,
            adapter,
            device,
            queue,
            size,
            needs_configure: true,
        })
    }

    /// Records the current size in physical pixels for the next redraw.
    pub fn resize(&mut self, size: [u32; 2]) {
        if self.size != size {
            self.size = size;
            self.needs_configure = true;
        }
    }

    /// Replaces the surface while its native source is still valid.
    pub fn recreate(&mut self) -> Result<(), RenderError> {
        drop(self.surface.take());
        self.surface = Some(create_surface(&self.instance, &self.source)?);
        self.needs_configure = true;
        Ok(())
    }

    fn configure(&self, surface: &wgpu::Surface<'_>) -> Result<(), RenderError> {
        let [width, height] = self.size;
        let limit = self.device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(RenderError(format!(
                "drawable size {width}x{height} exceeds the device limit of {limit}"
            )));
        }
        let config = surface
            .get_default_config(&self.adapter, width, height)
            .ok_or_else(|| RenderError("graphics adapter does not support the surface".into()))?;
        surface.configure(&self.device, &config);
        Ok(())
    }

    /// Clears the drawable to the given linear RGBA color and presents it.
    pub fn draw(&mut self, [r, g, b, a]: [f64; 4]) -> Result<DrawOutcome, RenderError> {
        if self.size.contains(&0) {
            return Ok(DrawOutcome::Idle);
        }
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| RenderError("graphics surface is unavailable".into()))?;
        if self.needs_configure {
            self.configure(surface)?;
            self.needs_configure = false;
        }

        let (frame, suboptimal) = match surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => (frame, false),
            CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            CurrentSurfaceTexture::Outdated => {
                self.needs_configure = true;
                return Ok(DrawOutcome::Retry);
            }
            CurrentSurfaceTexture::Lost => {
                self.recreate()?;
                return Ok(DrawOutcome::Retry);
            }
            CurrentSurfaceTexture::Timeout => return Ok(DrawOutcome::Retry),
            CurrentSurfaceTexture::Occluded => return Ok(DrawOutcome::Idle),
            CurrentSurfaceTexture::Validation => {
                return Err(RenderError(
                    "surface texture acquisition failed validation".into(),
                ));
            }
        };

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        self.queue.submit([encoder.finish()]);
        self.source.pre_present_notify();
        self.queue.present(frame);
        self.needs_configure = suboptimal;
        tracing::debug!(
            width = self.size[0],
            height = self.size[1],
            "presented frame"
        );
        Ok(DrawOutcome::Presented)
    }
}

fn create_surface(
    instance: &wgpu::Instance,
    source: &SurfaceSource,
) -> Result<wgpu::Surface<'static>, RenderError> {
    instance
        .create_surface(source.clone())
        .map_err(|error| RenderError(format!("could not create a graphics surface: {error}")))
}
