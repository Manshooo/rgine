//! Opt-in timings and draw workload for render attempts.

pub(crate) mod gpu;

use std::time::{Duration, Instant};

use serde::Serialize;

use self::gpu::PassQueries;

/// One measured CPU range on the monotonic process clock.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub name: &'static str,
    pub start: Instant,
    pub elapsed: Duration,
}

impl Span {
    pub fn since(name: &'static str, start: Instant) -> Self {
        Self {
            name,
            start,
            elapsed: start.elapsed(),
        }
    }
}

/// Work encoded for one render pass. First bindings in a pass count as changes.
#[derive(Debug, Default, Clone, Serialize)]
pub struct PassWork {
    pub name: &'static str,
    pub draw_calls: u64,
    pub instances: u64,
    pub triangles: u64,
    pub pipeline_changes: u64,
    pub bind_group_changes: u64,
    pub bind_group_sets: u64,
}

impl PassWork {
    pub(crate) fn draw(&mut self, triangles: u32) {
        self.draw_calls += 1;
        self.instances += 1;
        self.triangles += u64::from(triangles);
    }

    pub(crate) fn binding(&mut self, changed: bool) {
        self.bind_group_sets += 1;
        self.bind_group_changes += u64::from(changed);
    }
}

/// Graphics configuration associated with an attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Graphics {
    pub adapter: String,
    pub driver: String,
    pub backend: String,
    pub binding_mode: String,
    pub drawable_size: [u32; 2],
    pub surface_format: String,
    pub present_mode: String,
}

/// Queue writes captured between the start and end of a redraw attempt.
#[derive(Debug, Default)]
pub struct Uploads {
    pub bytes: u64,
    pub calls: u64,
    pub elapsed: Duration,
    pub spans: Vec<Span>,
    pub dropped_spans: u64,
}

impl Uploads {
    pub(crate) fn record(&mut self, size: usize, span: Span) {
        self.bytes += size as u64;
        self.calls += 1;
        self.elapsed += span.elapsed;
        if self.spans.len() < 24 {
            self.spans.push(span);
        } else {
            self.dropped_spans += 1;
        }
    }
}

/// Measurements from one render attempt, including acquisition failures.
#[derive(Debug, Default)]
pub struct RenderSample {
    pub spans: Vec<Span>,
    pub uploads: Uploads,
    pub passes: Vec<PassWork>,
    pub pipeline_creations: usize,
    pub acquisition: Option<&'static str>,
    pub presented_at: Option<Instant>,
    pub gpu: Option<GpuSample>,
}

/// GPU timestamps converted with the timestamp period of the submitting queue.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GpuSample {
    /// The device could not offer timestamp queries.
    Unavailable,
    /// All readback buffers were in flight when this frame was encoded.
    Dropped,
    /// Readback could not complete or did not provide valid timestamps.
    Incomplete,
    /// One duration for each render pass in the submission.
    Complete { passes: Vec<GpuPassTime> },
}

/// Timestamp interval relative to the earliest pass timestamp in the same frame.
#[derive(Debug, Serialize)]
pub struct GpuPassTime {
    pub name: &'static str,
    pub offset_ns: u64,
    pub duration_ns: u64,
}

#[derive(Default)]
pub(crate) struct PassProfile<'a, 'b> {
    pub sample: Option<&'a mut RenderSample>,
    pub queries: Option<PassQueries<'b>>,
}

impl PassProfile<'_, '_> {
    pub fn record(
        &mut self,
        name: &'static str,
    ) -> (
        Option<wgpu::RenderPassTimestampWrites<'_>>,
        Option<&mut PassWork>,
    ) {
        let timestamps = self
            .queries
            .as_mut()
            .and_then(|queries| queries.writes(name));
        let work = self.sample.as_mut().map(|sample| {
            sample.passes.push(PassWork {
                name,
                ..PassWork::default()
            });
            sample.passes.last_mut().expect("inserted pass")
        });
        (timestamps, work)
    }
}
