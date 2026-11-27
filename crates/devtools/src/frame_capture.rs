//! Chrome JSON timeline and statistical inspection of rgine frame captures.

use std::{collections::BTreeMap, error::Error};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Capture {
    schema_version: u32,
    build_profile: String,
    frame_limit: usize,
    omitted_attempts: u64,
    graphics: Vec<Value>,
    frames: Vec<Frame>,
}

#[derive(Deserialize)]
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
    outcome: String,
    acquisition: Option<String>,
    present_timestamp_ns: Option<u64>,
    present_interval_ns: Option<u64>,
    upload_bytes: u64,
    upload_calls: u64,
    upload_cpu_ns: u64,
    pipeline_creations: usize,
    passes: Vec<Pass>,
}

#[derive(Deserialize)]
struct Range {
    name: String,
    start_ns: u64,
    duration_ns: u64,
}

#[derive(Deserialize)]
struct Pass {
    name: String,
    #[serde(flatten)]
    work: Work,
}

#[derive(Deserialize, Serialize, Default)]
struct Work {
    draw_calls: u64,
    instances: u64,
    triangles: u64,
    pipeline_changes: u64,
    bind_group_changes: u64,
    bind_group_sets: u64,
}

impl Work {
    fn add(&mut self, other: &Self) {
        self.draw_calls += other.draw_calls;
        self.instances += other.instances;
        self.triangles += other.triangles;
        self.pipeline_changes += other.pipeline_changes;
        self.bind_group_changes += other.bind_group_changes;
        self.bind_group_sets += other.bind_group_sets;
    }
}

/// Inspects a raw frame capture or a replay report containing one.
/// Returns a Perfetto-compatible timeline and summary document.
pub fn inspect(mut input: Value) -> Result<(Value, Value), Box<dyn Error>> {
    let replay = input.get_mut("replay");
    let (raw, intervals) = if let Some(replay) = replay {
        let intervals = serde_json::from_value::<Vec<u64>>(replay["present_intervals_ns"].take())?;
        (replay["capture"].take(), Some(intervals))
    } else {
        (input.take(), None)
    };
    if raw.is_null() {
        let intervals = intervals.ok_or("expected a capture or a replay report")?;
        return Ok((
            json!({"traceEvents": []}),
            json!({"replay_present_ms": percentiles(intervals)}),
        ));
    }
    let capture: Capture = serde_json::from_value(raw)?;
    if capture.schema_version != 1 {
        return Err(format!(
            "unsupported frame capture schema {}",
            capture.schema_version
        )
        .into());
    }
    let mut events = vec![
        json!({"ph":"M","pid":1,"tid":1,"name":"thread_name","args":{"name":"Frame CPU"}}),
        json!({"ph":"M","pid":1,"tid":2,"name":"thread_name","args":{"name":"Frame counters"}}),
    ];
    let mut attempts = Vec::new();
    let mut presents = Vec::new();
    let mut ranges = BTreeMap::<&str, Vec<u64>>::new();
    let mut passes = BTreeMap::<&str, Work>::new();
    let mut outcomes = BTreeMap::<&str, usize>::new();
    let mut uploaded = 0;
    let mut pipelines = 0;
    let mut successful = 0;
    let mut caught_up = 0;
    let mut total_redraws = 0;
    let mut dropped_spans = 0;
    for frame in &capture.frames {
        *outcomes.entry(&frame.outcome).or_default() += 1;
        uploaded += frame.upload_bytes;
        pipelines += frame.pipeline_creations;
        caught_up += u64::from(frame.catch_up_steps);
        total_redraws += frame.redraw_requests;
        dropped_spans += frame.dropped_spans;
        attempts.push(frame.duration_ns);
        events.push(json!({
            "ph":"X", "pid":1, "tid":1, "cat":"frame", "name":"frame attempt",
            "ts":us(frame.start_ns), "dur":us(frame.duration_ns),
            "args": {"frame_id":frame.id,"outcome":frame.outcome,"graphics":frame.graphics,"acquisition":frame.acquisition}
        }));
        if let Some(present) = frame.present_timestamp_ns {
            successful += 1;
            events.push(json!({
                "ph":"i", "pid":1, "tid":2, "s":"t", "name":"frame presented",
                "ts":us(present),"args":{"frame_id":frame.id}
            }));
        }
        if let Some(interval) = frame.present_interval_ns {
            presents.push(interval);
            events.push(counter(
                "present interval (ms)",
                frame,
                json!({"ms":ms(interval)}),
            ));
        }
        events.push(counter("frame workload", frame, json!({
            "redraw_requests":frame.redraw_requests,"fixed_steps":frame.fixed_steps,
            "catch_up_steps":frame.catch_up_steps,"input_events":frame.input_events,
            "upload_bytes":frame.upload_bytes,"upload_calls":frame.upload_calls,
            "upload_cpu_us":us(frame.upload_cpu_ns),"pipeline_creations":frame.pipeline_creations
        })));
        let mut frame_ranges = BTreeMap::<&str, u64>::new();
        for span in &frame.spans {
            events.push(json!({
                "ph":"X","pid":1,"tid":1,"cat":"cpu","name":span.name,
                "ts":us(span.start_ns),"dur":us(span.duration_ns),"args":{"frame_id":frame.id}
            }));
            *frame_ranges.entry(&span.name).or_default() += span.duration_ns;
        }
        for (name, elapsed) in frame_ranges {
            ranges.entry(name).or_default().push(elapsed);
        }
        for pass in &frame.passes {
            passes.entry(&pass.name).or_default().add(&pass.work);
            events.push(counter(
                &pass.name,
                frame,
                serde_json::to_value(&pass.work)?,
            ));
        }
    }
    let ranges: BTreeMap<_, _> = ranges
        .into_iter()
        .map(|(name, values)| (name, percentiles(values)))
        .collect();
    let summary = json!({
        "build_profile":capture.build_profile,"graphics":capture.graphics,
        "frame_limit":capture.frame_limit,"omitted_attempts":capture.omitted_attempts,
        "dropped_spans":dropped_spans,"outcomes":outcomes,"successful_presents":successful,
        "redraw_requests":total_redraws,"catch_up_steps":caught_up,
        "attempt_elapsed_ms":percentiles(attempts),"present_interval_ms":percentiles(presents),
        "replay_present_ms":intervals.and_then(percentiles),"cpu_ms_by_range":ranges,
        "upload_bytes":uploaded,"pipeline_creations":pipelines,"pass_totals":passes
    });
    Ok((
        json!({"traceEvents":events, "displayTimeUnit":"ms"}),
        summary,
    ))
}

fn counter(name: &str, frame: &Frame, args: Value) -> Value {
    json!({"ph":"C","pid":1,"tid":2,"ts":us(frame.start_ns),"name":name,"args":args})
}

fn us(ns: u64) -> f64 {
    ns as f64 / 1_000.0
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

#[derive(Serialize)]
struct Percentiles {
    samples: usize,
    median_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
}

fn percentiles(mut values: Vec<u64>) -> Option<Percentiles> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let size = values.len();
    let median = (ms(values[(size - 1) / 2]) + ms(values[size / 2])) * 0.5;
    let nearest = |percent| ms(values[usize::div_ceil(size * percent, 100) - 1]);
    Some(Percentiles {
        samples: size,
        median_ms: median,
        p95_ms: nearest(95),
        p99_ms: nearest(99),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_correlated_slices_and_separate_present_statistics() {
        let frame = |id: usize, outcome: &str, present: Option<u64>, interval: Option<u64>| {
            json!({
                "id":id, "outcome":outcome, "start_ns":id * 20_000_000, "duration_ns":5_000_000,
                "graphics":null, "spans":[{"name":"fixed step","start_ns":id * 20_000_000,"duration_ns":1_000_000}],
                "dropped_spans":0, "redraw_requests":1,"fixed_steps":1,"catch_up_steps":0,"input_events":1,
                "acquisition":outcome,"present_timestamp_ns":present,"present_interval_ns":interval,
                "upload_bytes":16,"upload_calls":1,"upload_cpu_ns":10_000,"pipeline_creations":0,
                "passes":[{"name":"opaque scene","draw_calls":1,"instances":1,"triangles":2,
                          "pipeline_changes":1,"bind_group_changes":1,"bind_group_sets":2}]
            })
        };
        let (trace, summary) = inspect(json!({
            "schema_version":1,"build_profile":"debug","frame_limit":3,"omitted_attempts":1,
            "graphics":[],"frames":[
                frame(1,"presented",Some(22_000_000),None), frame(2,"retry",None,None),
                frame(3,"presented",Some(62_000_000),Some(40_000_000))
            ]
        }))
        .unwrap();
        assert_eq!(summary["successful_presents"], 2);
        assert_eq!(summary["outcomes"]["retry"], 1);
        assert_eq!(summary["attempt_elapsed_ms"]["median_ms"], 5.0);
        assert_eq!(summary["present_interval_ms"]["p99_ms"], 40.0);
        assert_eq!(summary["pass_totals"]["opaque scene"]["triangles"], 6);
        let events = trace["traceEvents"].as_array().unwrap();
        assert!(events.iter().any(|e| e["name"] == "fixed step"
            && e["args"]["frame_id"] == 3
            && e["dur"] == 1000.0));
        assert!(
            events
                .iter()
                .any(|e| e["name"] == "opaque scene" && e["ph"] == "C")
        );
    }
}
