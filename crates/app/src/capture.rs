//! Bounded frame capture on the application thread.

use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use engine_render::profile::{Graphics, PassWork, RenderSample, Span};
use serde::Serialize;

use crate::InputRecording;

const MAX_FRAMES: usize = 10_000;
const MAX_SPANS: usize = 64;

/// Maximum stored attempts. Collection stops at this limit.
#[derive(Debug, Clone, Copy)]
pub struct CaptureLimit(NonZeroUsize);

impl CaptureLimit {
    /// Accepts between 1 and 10,000 attempts.
    pub fn new(frames: usize) -> Option<Self> {
        NonZeroUsize::new(frames)
            .filter(|frames| frames.get() <= MAX_FRAMES)
            .map(Self)
    }
}

/// Raw capture data for the frame inspector in engine-devtools.
#[derive(Debug, Serialize)]
pub struct FrameCapture {
    schema_version: u32,
    build_profile: &'static str,
    frame_limit: usize,
    omitted_attempts: u64,
    graphics: Vec<Graphics>,
    frames: Vec<Frame>,
    #[serde(skip)]
    epoch: Instant,
    #[serde(skip)]
    previous_present: Option<Instant>,
    #[serde(skip)]
    pub(crate) pending: Pending,
}

#[derive(Debug, Serialize)]
struct Frame {
    id: usize,
    start_ns: u64,
    duration_ns: u64,
    graphics: Option<usize>,
    spans: Vec<Range>,
    dropped_spans: u64,
    redraw_requests: usize,
    fixed_steps: u32,
    catch_up_steps: u32,
    input_events: usize,
    outcome: &'static str,
    acquisition: Option<&'static str>,
    present_timestamp_ns: Option<u64>,
    present_interval_ns: Option<u64>,
    upload_bytes: u64,
    upload_calls: u64,
    upload_cpu_ns: u64,
    pipeline_creations: usize,
    passes: Vec<PassWork>,
}

#[derive(Debug, Serialize)]
struct Range {
    name: &'static str,
    start_ns: u64,
    duration_ns: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Pending {
    pub start: Option<Instant>,
    pub spans: Spans,
    pub redraw_requests: usize,
    pub fixed_steps: u32,
    pub catch_up_steps: u32,
    pub input_events: usize,
    render: Option<RenderSample>,
    graphics: Option<Graphics>,
}

#[derive(Debug, Default)]
pub(crate) struct Spans {
    spans: Vec<Span>,
    dropped: u64,
}

impl Spans {
    pub fn push(&mut self, name: &'static str, start: Instant) {
        self.record(Span::since(name, start));
    }

    fn record(&mut self, span: Span) {
        if self.spans.len() < MAX_SPANS {
            self.spans.push(span);
        } else {
            self.dropped += 1;
        }
    }
}

impl Pending {
    pub fn render(&mut self, sample: Option<RenderSample>, graphics: Option<Graphics>) {
        self.render = sample;
        self.graphics = graphics;
    }
}

impl FrameCapture {
    pub(crate) fn new(limit: CaptureLimit) -> Self {
        Self {
            schema_version: 1,
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            frame_limit: limit.0.get(),
            omitted_attempts: 0,
            graphics: Vec::new(),
            frames: Vec::with_capacity(limit.0.get()),
            epoch: Instant::now(),
            previous_present: None,
            pending: Pending::default(),
        }
    }

    pub(crate) fn pending(&mut self) -> Option<&mut Pending> {
        (self.frames.len() < self.frame_limit).then_some(&mut self.pending)
    }

    pub(crate) fn finish(&mut self, outcome: &'static str, end: Instant) {
        if self.frames.len() == self.frame_limit {
            self.omitted_attempts += 1;
            return;
        }
        let mut pending = std::mem::take(&mut self.pending);
        let render = pending.render.take().unwrap_or_default();
        for span in render.spans.into_iter().chain(render.uploads.spans) {
            pending.spans.record(span);
        }
        let timestamp = render.presented_at.map(|at| self.timestamp(at));
        let interval = render.presented_at.and_then(|at| {
            self.previous_present
                .replace(at)
                .map(|previous| nanos(at.duration_since(previous)))
        });
        let graphics = pending.graphics.map(|graphics| {
            self.graphics
                .iter()
                .position(|configuration| configuration == &graphics)
                .unwrap_or_else(|| {
                    self.graphics.push(graphics);
                    self.graphics.len() - 1
                })
        });
        let start = pending.start.unwrap_or(end);
        let spans = pending
            .spans
            .spans
            .into_iter()
            .map(|span| Range {
                name: span.name,
                start_ns: self.timestamp(span.start),
                duration_ns: nanos(span.elapsed),
            })
            .collect();
        self.frames.push(Frame {
            id: self.frames.len() + 1,
            start_ns: self.timestamp(start),
            duration_ns: nanos(end.saturating_duration_since(start)),
            graphics,
            spans,
            dropped_spans: pending.spans.dropped + render.uploads.dropped_spans,
            redraw_requests: pending.redraw_requests,
            fixed_steps: pending.fixed_steps,
            catch_up_steps: pending.catch_up_steps,
            input_events: pending.input_events,
            outcome,
            acquisition: render.acquisition,
            present_timestamp_ns: timestamp,
            present_interval_ns: interval,
            upload_bytes: render.uploads.bytes,
            upload_calls: render.uploads.calls,
            upload_cpu_ns: nanos(render.uploads.elapsed),
            pipeline_creations: render.pipeline_creations,
            passes: render.passes,
        });
    }

    fn timestamp(&self, at: Instant) -> u64 {
        nanos(at.saturating_duration_since(self.epoch))
    }
}

/// Data from a deterministic scene replay, with the same timing in both modes.
#[derive(Debug, Serialize)]
pub struct ReplayReport {
    pub warmup_frames: usize,
    pub presented_frames: usize,
    pub present_intervals_ns: Vec<u64>,
    pub capture: Option<FrameCapture>,
}

pub(crate) struct Replay {
    pub recording: InputRecording,
    pub warmup: usize,
    pub presented: usize,
    pub stepped: bool,
    pub previous_present: Option<Instant>,
    pub intervals: Vec<u64>,
}

impl Replay {
    pub fn present(&mut self, now: Instant) {
        if self.presented >= self.warmup
            && let Some(previous) = self.previous_present
        {
            self.intervals.push(nanos(now.duration_since(previous)));
        }
        self.previous_present = Some(now);
        self.presented += 1;
        self.stepped = false;
    }
}

pub(crate) fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

#[derive(Default)]
pub(crate) struct Session {
    pub capture: Option<FrameCapture>,
    pub replay: Option<Replay>,
}

impl Session {
    pub fn capture_attempt(&mut self) -> Option<&mut FrameCapture> {
        let measured = self
            .replay
            .as_ref()
            .is_none_or(|replay| replay.presented >= replay.warmup);
        self.capture.as_mut().filter(|_| measured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_first_attempts_and_times_only_successful_presents() {
        assert!(CaptureLimit::new(0).is_none());
        assert!(CaptureLimit::new(MAX_FRAMES + 1).is_none());
        let mut capture = FrameCapture::new(CaptureLimit::new(3).unwrap());
        let epoch = capture.epoch;
        for (index, outcome) in ["presented", "retry", "presented", "idle"]
            .into_iter()
            .enumerate()
        {
            let at = epoch + Duration::from_millis(index as u64 * 20);
            if let Some(pending) = capture.pending() {
                pending.start = Some(at);
                for _ in 0..=MAX_SPANS {
                    pending.spans.record(Span {
                        name: "input",
                        start: at,
                        elapsed: Duration::from_micros(2),
                    });
                }
                let timestamp = (outcome == "presented").then_some(at + Duration::from_millis(2));
                pending.render(
                    Some(RenderSample {
                        presented_at: timestamp,
                        ..Default::default()
                    }),
                    None,
                );
            }
            capture.finish(outcome, at + Duration::from_millis(5));
        }
        let json = serde_json::to_value(capture).unwrap();
        let frames = json["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(json["omitted_attempts"], 1);
        assert_eq!(frames[0]["spans"].as_array().unwrap().len(), MAX_SPANS);
        assert_eq!(frames[0]["dropped_spans"], 1);
        assert!(frames[1]["present_interval_ns"].is_null());
        assert_eq!(frames[2]["id"], 3);
        assert_eq!(frames[2]["present_interval_ns"], 40_000_000);
    }
}
