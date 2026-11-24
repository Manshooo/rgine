mod backend;

use std::fmt;

use engine_platform::SurfaceSource;
use wgpu::CurrentSurfaceTexture;

use crate::graph::{CompiledGraph, GraphError, RenderGraph};
use crate::resources::{ResourceError, ResourceTable};
use crate::scene::{DirectionalLight, MeshRegistry, Scene, SceneRenderer};

/// A GPU or surface failure reported without exposing backend types.
#[derive(Debug)]
pub struct RenderError(pub(crate) String);

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RenderError {}

impl From<ResourceError> for RenderError {
    fn from(error: ResourceError) -> Self {
        Self(error.to_string())
    }
}

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
    resources: ResourceTable,
    frame_graph: CompiledGraph<()>,
    scene: SceneRenderer,
    size: [u32; 2],
    needs_configure: bool,
}

impl Drawable {
    /// Initializes the GPU backend for a live surface. Call outside native
    /// lifecycle callbacks, since adapter and device creation can block.
    pub fn new(source: SurfaceSource, size: [u32; 2]) -> Result<Self, RenderError> {
        let frame_graph = clear_graph().map_err(|error| RenderError(error.to_string()))?;
        let backend::Graphics {
            surface,
            instance,
            adapter,
            resources,
        } = backend::open(&source)?;
        let info = adapter.get_info();
        tracing::info!(
            adapter = %info.name,
            backend = ?info.backend,
            binding_mode = ?resources.binding_mode(),
            "graphics device ready"
        );
        Ok(Self {
            surface: Some(surface),
            source,
            instance,
            adapter,
            resources,
            frame_graph,
            scene: SceneRenderer::default(),
            size,
            needs_configure: true,
        })
    }

    /// Accesses this device's resource table before preparing the next frame.
    pub fn resources(&mut self) -> &mut ResourceTable {
        &mut self.resources
    }

    /// Sets the direct light for subsequent scene draws.
    pub fn set_directional_light(&mut self, light: DirectionalLight) {
        self.scene.light = light;
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
        let limit = self.resources.device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(RenderError(format!(
                "drawable size {width}x{height} exceeds the device limit of {limit}"
            )));
        }
        let config = surface
            .get_default_config(&self.adapter, width, height)
            .ok_or_else(|| RenderError("graphics adapter does not support the surface".into()))?;
        surface.configure(&self.resources.device, &config);
        Ok(())
    }

    /// Clears the drawable to the given linear RGBA color and presents it.
    pub fn draw(&mut self, clear: [f64; 4]) -> Result<DrawOutcome, RenderError> {
        self.present(clear, None)
    }

    /// Draws the extracted scene with the current device and drawable size.
    pub fn draw_scene(
        &mut self,
        clear: [f64; 4],
        meshes: &MeshRegistry,
        scene: &Scene,
    ) -> Result<DrawOutcome, RenderError> {
        self.present(clear, Some((meshes, scene)))
    }

    fn present(
        &mut self,
        [r, g, b, a]: [f64; 4],
        scene: Option<(&MeshRegistry, &Scene)>,
    ) -> Result<DrawOutcome, RenderError> {
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
        let mut encoder = self
            .resources
            .device
            .create_command_encoder(&Default::default());
        let color = wgpu::Color { r, g, b, a };
        if let Some((meshes, scene)) = scene {
            let size = [frame.texture.width(), frame.texture.height()];
            self.scene.prepare(
                &mut self.resources,
                meshes,
                scene,
                size,
                frame.texture.format(),
            )?;
            self.scene
                .encode(&self.resources, scene, &mut encoder, &view, color)?;
        } else {
            self.encode_frame(&mut encoder, &view, color)?;
        }
        self.resources.queue.submit([encoder.finish()]);
        self.source.pre_present_notify();
        self.resources.queue.present(frame);
        self.needs_configure = suboptimal;
        tracing::debug!(
            width = self.size[0],
            height = self.size[1],
            "presented frame"
        );
        Ok(DrawOutcome::Presented)
    }

    fn encode_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        color: wgpu::Color,
    ) -> Result<(), RenderError> {
        let [backbuffer] = self.frame_graph.exports() else {
            return Err(RenderError(
                "the frame graph must export one backbuffer".into(),
            ));
        };
        self.frame_graph.execute(|pass| {
            if pass.writes() != [*backbuffer] {
                return Err(RenderError(format!(
                    "frame pass {:?} does not target the backbuffer",
                    pass.name()
                )));
            }
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(pass.name()),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            }));
            Ok(())
        })
    }
}

fn clear_graph() -> Result<CompiledGraph<()>, GraphError> {
    let mut graph = RenderGraph::new();
    let backbuffer = graph.import("backbuffer");
    graph.add_pass("clear backbuffer", &[], &[backbuffer], ());
    graph.export(backbuffer);
    graph.compile()
}

fn create_surface(
    instance: &wgpu::Instance,
    source: &SurfaceSource,
) -> Result<wgpu::Surface<'static>, RenderError> {
    instance
        .create_surface(source.clone())
        .map_err(|error| RenderError(format!("could not create a graphics surface: {error}")))
}
