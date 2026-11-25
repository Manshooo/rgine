use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use engine_core::glam::{Mat4, Vec3};
use engine_render::scene::Scene;
use engine_render::{BenchmarkGraphics, DrawOutcome, Drawable};
use serde::Serialize;

use super::{RenderError, SceneFrames};

pub(super) const DRAWABLE_SIZE: [u32; 2] = [1280, 720];
const WARMUP_FRAMES: u32 = 120;
const ORBIT_TIME: Duration = Duration::from_secs(20);
const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;

/// Configuration and frame delivery measurements from a completed Render v1 run.
#[derive(Debug, Serialize)]
pub struct BenchmarkReport {
    adapter: String,
    driver: String,
    backend: &'static str,
    binding_mode: &'static str,
    present_mode: &'static str,
    surface_format: String,
    drawable_size: [u32; 2],
    pipeline_keys: Vec<String>,
    pipelines_prepared: usize,
    warmup_pipeline_creations: usize,
    measured_pipeline_creations: usize,
    drawable_startup_ms: f64,
    pipeline_preparation_ms: f64,
    warmup_frames: u32,
    measured_frames: usize,
    elapsed_ms: f64,
    gpu_drain_ms: f64,
    frames_per_second: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    passed: bool,
    frame_intervals_ms: Vec<f64>,
}

impl BenchmarkReport {
    /// Whether throughput and both frame-time thresholds passed with no late pipeline creation.
    pub fn passed(&self) -> bool {
        self.passed
    }
}

pub(super) struct Benchmark {
    expected_adapter: String,
    graphics: Option<BenchmarkGraphics>,
    startup: Duration,
    preparation: Duration,
    warmup: u32,
    orbit_start: Option<Instant>,
    previous_present: Option<Instant>,
    intervals: Vec<f64>,
    pub(super) interrupted: bool,
    pub(super) report: Option<BenchmarkReport>,
}

impl Benchmark {
    pub(super) fn new(expected_adapter: String) -> Self {
        Self {
            expected_adapter,
            graphics: None,
            startup: Duration::ZERO,
            preparation: Duration::ZERO,
            warmup: 0,
            orbit_start: None,
            previous_present: None,
            intervals: Vec::with_capacity(4096),
            interrupted: false,
            report: None,
        }
    }

    pub(super) fn started(&self) -> bool {
        self.warmup != 0
    }

    pub(super) fn prepare(
        &mut self,
        drawable: &Drawable,
        scene: &SceneFrames,
        startup: Duration,
        preparation: Duration,
    ) -> Result<(), RenderError> {
        if self.started() || !scene.skybox_enabled || scene.snapshot.objects.is_empty() {
            return Err(RenderError::Benchmark(
                "benchmark requires the original drawable, opaque geometry and the skybox".into(),
            ));
        }
        let graphics = drawable
            .benchmark_graphics()
            .expect("benchmark configured before preparation");
        if graphics.adapter != self.expected_adapter {
            return Err(RenderError::Benchmark(format!(
                "expected adapter {:?}; selected {:?}",
                self.expected_adapter, graphics.adapter
            )));
        }
        self.graphics = Some(graphics);
        self.startup = startup;
        self.preparation = preparation;
        Ok(())
    }

    pub(super) fn pose(&self, scene: &mut Scene) -> Result<bool, RenderError> {
        let camera = scene
            .camera
            .as_mut()
            .ok_or_else(|| RenderError::Benchmark("benchmark scene requires a camera".into()))?;
        let elapsed = self
            .orbit_start
            .map_or(Duration::ZERO, |start| start.elapsed());
        let u = (elapsed.as_secs_f64() / ORBIT_TIME.as_secs_f64()).min(1.0) as f32;
        camera.view = orbit_view(u);
        Ok(elapsed >= ORBIT_TIME)
    }

    pub(super) fn present(
        &mut self,
        drawable: &Drawable,
        outcome: DrawOutcome,
        end_of_orbit: bool,
    ) -> Result<bool, RenderError> {
        if outcome != DrawOutcome::Presented {
            return Err(RenderError::Benchmark(format!(
                "benchmark present did not complete ({outcome:?})"
            )));
        }
        let during_warmup = self.warmup < WARMUP_FRAMES;
        let graphics = self
            .graphics
            .as_ref()
            .expect("graphics measured after preparation");
        let prepared = graphics.pipelines.len();
        if drawable.scene_pipeline_creations() != prepared {
            let phase = if during_warmup {
                "warmup"
            } else {
                "the measured pass"
            };
            let late = drawable.scene_pipeline_creations() - prepared;
            return Err(RenderError::Benchmark(format!(
                "created {late} pipelines during {phase}"
            )));
        }
        let now = drawable
            .last_benchmark_present()
            .expect("benchmark present was timestamped");
        let previous = self.previous_present.replace(now);
        if during_warmup {
            self.warmup += 1;
            if self.warmup == WARMUP_FRAMES {
                self.orbit_start = Some(now);
            }
            return Ok(false);
        }
        self.intervals.push(ms(
            now - previous.expect("warmup established a present timestamp")
        ));
        if !end_of_orbit {
            return Ok(false);
        }

        let drain = drawable.drain().map_err(RenderError::Graphics)?;
        let elapsed = now - self.orbit_start.expect("warmup established an orbit start") + drain;
        let graphics = self
            .graphics
            .take()
            .expect("graphics measured after preparation");
        let intervals = std::mem::take(&mut self.intervals);
        let mut sorted = intervals.clone();
        sorted.sort_unstable_by(f64::total_cmp);
        let p99 = rank(&sorted, 99);
        let max = sorted[sorted.len() - 1];
        let fps = intervals.len() as f64 / elapsed.as_secs_f64();
        self.report = Some(BenchmarkReport {
            adapter: graphics.adapter,
            driver: graphics.driver,
            backend: "DX12",
            binding_mode: "Classic",
            present_mode: "Immediate",
            surface_format: graphics.format,
            drawable_size: DRAWABLE_SIZE,
            pipeline_keys: graphics.pipelines,
            pipelines_prepared: prepared,
            warmup_pipeline_creations: 0,
            measured_pipeline_creations: 0,
            drawable_startup_ms: ms(self.startup),
            pipeline_preparation_ms: ms(self.preparation),
            warmup_frames: self.warmup,
            measured_frames: intervals.len(),
            elapsed_ms: ms(elapsed),
            gpu_drain_ms: ms(drain),
            frames_per_second: fps,
            p50_ms: rank(&sorted, 50),
            p95_ms: rank(&sorted, 95),
            p99_ms: p99,
            max_ms: max,
            passed: fps >= 60.0 && p99 <= FRAME_BUDGET_MS && max <= 2.0 * FRAME_BUDGET_MS,
            frame_intervals_ms: intervals,
        });
        Ok(true)
    }
}

fn orbit_view(u: f32) -> Mat4 {
    let azimuth = TAU * u;
    let (sin, cos) = azimuth.sin_cos();
    let elevation = 0.35 + 0.2 * sin;
    let radius = 6.2 + 1.2 * sin;
    let horizontal = radius * elevation.cos();
    let target = Vec3::new(0.0, 0.6, 0.0);
    let position = target + Vec3::new(horizontal * sin, radius * elevation.sin(), horizontal * cos);
    Mat4::look_at_rh(position, target, Vec3::Y)
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn rank(sorted: &[f64], percentile: usize) -> f64 {
    sorted[(sorted.len() * percentile).div_ceil(100) - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_includes_the_last_of_small_samples() {
        assert_eq!(rank(&[1.0, 2.0, 3.0, 4.0], 50), 2.0);
        assert_eq!(rank(&[1.0, 2.0, 3.0, 4.0], 99), 4.0);
        let samples: Vec<_> = (1..=100).map(f64::from).collect();
        assert_eq!(rank(&samples, 99), 99.0);
    }

    #[test]
    fn orbit_starts_on_positive_z_and_returns_to_the_starting_pose() {
        let eye = |u| orbit_view(u).inverse().transform_point3(Vec3::ZERO);
        let target = Vec3::new(0.0, 0.6, 0.0);
        let start = eye(0.0);
        assert!(start.z > 0.0 && start.x.abs() < 1e-5);
        assert!((start - eye(1.0)).length() < 1e-5);
        assert!(((eye(0.25) - target).length() - 7.4).abs() < 1e-5);
        assert!(((eye(0.75) - target).length() - 5.0).abs() < 1e-5);
    }
}
