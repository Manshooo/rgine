mod backend;

use std::fmt;
use std::time::{Duration, Instant};

use engine_platform::SurfaceSource;
use wgpu::CurrentSurfaceTexture;

use crate::graph::{CompiledGraph, GraphError, RenderGraph};
use crate::resources::{BindingMode, ResourceError, ResourceTable};
use crate::scene::{
    DirectionalLight, HemisphereLight, MeshRegistry, SCENE_COLOR_FORMATS, Scene, SceneRenderer,
};

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

/// The GPU and pipeline configuration validated for the Render v1 benchmark.
#[derive(Debug)]
pub struct BenchmarkGraphics {
    /// Name of the selected adapter.
    pub adapter: String,
    /// Driver version or backend-specific driver information.
    pub driver: String,
    /// Configured SDR surface format.
    pub format: String,
    /// Full keys of the scene pipelines prepared on this device.
    pub pipelines: Vec<String>,
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
    surface_format: Option<wgpu::TextureFormat>,
    immediate: bool,
    last_present: Option<Instant>,
}

impl Drawable {
    /// Initializes the GPU backend for a live surface. Call outside native
    /// lifecycle callbacks, since adapter and device creation can block.
    pub fn new(source: SurfaceSource, size: [u32; 2]) -> Result<Self, RenderError> {
        Self::open(source, size, false)
    }

    /// Opens a drawable that requires Immediate presentation, DX12 and classic bindings.
    pub fn new_for_benchmark(source: SurfaceSource, size: [u32; 2]) -> Result<Self, RenderError> {
        Self::open(source, size, true)
    }

    fn open(source: SurfaceSource, size: [u32; 2], immediate: bool) -> Result<Self, RenderError> {
        let frame_graph = clear_graph().map_err(|error| RenderError(error.to_string()))?;
        let backend::Graphics {
            surface,
            instance,
            adapter,
            resources,
        } = backend::open(&source)?;
        let info = adapter.get_info();
        if immediate
            && (info.backend != wgpu::Backend::Dx12
                || resources.binding_mode() != BindingMode::Classic)
        {
            return Err(RenderError(format!(
                "benchmark requires DX12 and classic bindings; selected {:?} with {:?}",
                info.backend,
                resources.binding_mode()
            )));
        }
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
            surface_format: None,
            immediate,
            last_present: None,
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

    /// Sets the upper and lower diffuse lighting for subsequent scene draws.
    pub fn set_hemisphere_light(&mut self, light: HemisphereLight) {
        self.scene.hemisphere_light = light;
    }

    /// Draws the procedural sky behind opaque geometry when a scene has a camera.
    pub fn set_skybox_enabled(&mut self, enabled: bool) {
        self.scene.skybox_enabled = enabled;
    }

    /// Prepares scene pipelines for the current device, drawable format and skybox setting.
    /// Call after configuring the scene and before starting a timed render loop with a nonzero size.
    pub fn prepare_scene_pipelines(&mut self) -> Result<(), RenderError> {
        if self.size.contains(&0) {
            return Err(RenderError(
                "cannot prepare a zero-sized scene drawable".into(),
            ));
        }
        let format = self.configure_if_needed()?;
        self.scene.prepare_pipelines(&mut self.resources, format)
    }

    /// Rebuilds affected scene pipelines and publishes them after every candidate validates.
    /// Returns whether any currently prepared pipeline changed.
    #[cfg(feature = "shader-reload")]
    pub fn reload_scene_shaders(
        &mut self,
        sources: &crate::SceneShaderSources,
    ) -> Result<bool, RenderError> {
        self.prepare_scene_pipelines()?;
        self.scene.reload_shaders(&mut self.resources, sources)
    }

    /// Counts successful scene pipeline creations on this drawable's device.
    /// Take the difference between samples before and after a measured pass.
    pub fn scene_pipeline_creations(&self) -> usize {
        self.scene.pipeline_creations()
    }

    /// Returns the active graphics configuration after preparing the benchmark pipelines.
    pub fn benchmark_graphics(&self) -> Option<BenchmarkGraphics> {
        let format = self.surface_format.filter(|_| self.immediate)?;
        let info = self.adapter.get_info();
        Some(BenchmarkGraphics {
            adapter: info.name,
            driver: format!("{} {}", info.driver, info.driver_info)
                .trim()
                .into(),
            format: format!("{format:?}"),
            pipelines: self.scene.pipeline_keys(),
        })
    }

    /// Returns the monotonic timestamp captured at return from the last benchmark present.
    pub fn last_benchmark_present(&self) -> Option<Instant> {
        self.last_present
    }

    /// Waits once for work submitted on this drawable and returns the wait duration.
    pub fn drain(&self) -> Result<Duration, RenderError> {
        let start = Instant::now();
        self.resources
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| RenderError(format!("could not drain the graphics queue: {error}")))?;
        Ok(start.elapsed())
    }

    /// Records the current size in physical pixels for the next redraw.
    pub fn resize(&mut self, size: [u32; 2]) {
        if self.size != size {
            self.size = size;
            self.surface_format = None;
        }
    }

    /// Replaces the surface while its native source is still valid.
    pub fn recreate(&mut self) -> Result<(), RenderError> {
        self.surface_format = None;
        drop(self.surface.take());
        self.surface = Some(create_surface(&self.instance, &self.source)?);
        Ok(())
    }

    fn configure_if_needed(&mut self) -> Result<wgpu::TextureFormat, RenderError> {
        if let Some(format) = self.surface_format {
            return Ok(format);
        }
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| RenderError("graphics surface is unavailable".into()))?;
        let [width, height] = self.size;
        let limit = self.resources.device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(RenderError(format!(
                "drawable size {width}x{height} exceeds the device limit of {limit}"
            )));
        }
        let mut config = surface
            .get_default_config(&self.adapter, width, height)
            .ok_or_else(|| RenderError("graphics adapter does not support the surface".into()))?;
        if self.immediate {
            let caps = surface.get_capabilities(&self.adapter);
            if !caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
                return Err(RenderError(
                    "graphics surface does not support Immediate presentation".into(),
                ));
            }
            config.present_mode = wgpu::PresentMode::Immediate;
            config.format = caps
                .formats
                .into_iter()
                .find(|format| {
                    matches!(
                        format,
                        wgpu::TextureFormat::Bgra8UnormSrgb
                            | wgpu::TextureFormat::Bgra8Unorm
                            | wgpu::TextureFormat::Rgba8UnormSrgb
                            | wgpu::TextureFormat::Rgba8Unorm
                    )
                })
                .ok_or_else(|| RenderError("graphics surface offers no 8-bit SDR format".into()))?;
        } else if !SCENE_COLOR_FORMATS.contains(&config.format) {
            config.format = surface
                .get_capabilities(&self.adapter)
                .formats
                .into_iter()
                .find(|format| SCENE_COLOR_FORMATS.contains(format))
                .ok_or_else(|| RenderError("surface offers no supported color format".into()))?;
        }
        surface.configure(&self.resources.device, &config);
        self.surface_format = Some(config.format);
        Ok(config.format)
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
        let pipelines_before = self.scene_pipeline_creations();
        let format = self.configure_if_needed()?;
        if scene.is_some() {
            self.scene.prepare_pipelines(&mut self.resources, format)?;
        }
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| RenderError("graphics surface is unavailable".into()))?;

        let (frame, suboptimal) = match surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => (frame, false),
            CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            CurrentSurfaceTexture::Outdated => {
                self.surface_format = None;
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
            self.scene
                .prepare(&mut self.resources, meshes, scene, size)?;
            self.scene
                .encode(&self.resources, scene, &mut encoder, &view, format, color)?;
        } else {
            self.encode_frame(&mut encoder, &view, color)?;
        }
        self.resources.queue.submit([encoder.finish()]);
        self.source.pre_present_notify();
        self.resources.queue.present(frame);
        if self.immediate {
            self.last_present = Some(Instant::now());
        }
        if suboptimal {
            self.surface_format = None;
            if self.immediate {
                return Ok(DrawOutcome::Retry);
            }
        }
        tracing::debug!(
            width = self.size[0],
            height = self.size[1],
            pipeline_creations = self.scene_pipeline_creations() - pipelines_before,
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
