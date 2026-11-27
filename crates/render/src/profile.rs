//! Opt-in CPU timings and draw workload for one render attempt.

use std::time::{Duration, Instant};

use serde::Serialize;

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
}

impl RenderSample {
    pub(crate) fn pass(&mut self, name: &'static str) -> &mut PassWork {
        self.passes.push(PassWork {
            name,
            ..PassWork::default()
        });
        self.passes.last_mut().expect("inserted pass")
    }
}
