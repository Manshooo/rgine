use std::error::Error;
use std::ffi::OsString;
use std::io::BufWriter;
use std::time::Duration;

use engine_app::InputRecording;
use engine_app::capture::CaptureLimit;
use engine_app::platform::{
    Commands, Input, InputEvent, InputTimestamp, KeyCode, PhysicalKey, WindowDesc,
};

use super::{arena_with_hud, visuals::HudMode};

const STEP: Duration = Duration::from_nanos(16_666_667);
const WARMUP: usize = 30;
const MEASURED: usize = 240;

pub(super) fn run(
    mut args: impl Iterator<Item = OsString>,
    clear: [f64; 4],
) -> Result<(), Box<dyn Error>> {
    let usage =
        "usage: deflection_arena --profile <original|batched|hidden> <off|on|gpu> <report.json>";
    let hud = args
        .next()
        .and_then(|arg| arg.into_string().ok())
        .ok_or(usage)?;
    let mode = match hud.as_str() {
        "original" => HudMode::Original,
        "batched" => HudMode::Batched,
        "hidden" => HudMode::Hidden,
        _ => return Err(usage.into()),
    };
    let capture_mode = args.next().ok_or(usage)?;
    let capture = match capture_mode.to_str() {
        Some("on") => CaptureLimit::new(MEASURED * 2),
        Some("gpu") => CaptureLimit::new(MEASURED * 2).map(CaptureLimit::with_gpu_timing),
        Some("off") => None,
        _ => return Err(usage.into()),
    };
    let output = args.next().ok_or(usage)?;
    if args.next().is_some() {
        return Err(usage.into());
    }
    let recording = recording();
    let (app, meshes, _) = arena_with_hud(mode);
    let replay = app.run_scene_replay(meshes, clear, recording, WARMUP, capture)?;
    let report = serde_json::json!({
        "scenario": "deflection-arena-v1",
        "hud": hud,
        "cpu_capture": capture.is_some(),
        "replay": replay,
    });
    let mut writer = BufWriter::new(std::fs::File::create(output)?);
    serde_json::to_writer(&mut writer, &report)?;
    std::io::Write::flush(&mut writer)?;
    Ok(())
}

pub(super) fn recording() -> InputRecording {
    use KeyCode::*;
    let (app, _, _) = arena_with_hud(HudMode::Hidden);
    let mut sim = app.into_simulation();
    let mut windows = Vec::new();
    Commands::default().create_windows(&[WindowDesc::default()], &mut windows);
    let window = windows[0];
    let inputs = [
        (71, Space, true),
        (72, Space, false),
        (105, KeyD, true),
        (120, KeyD, false),
        (144, Escape, true),
        (145, Escape, false),
        (157, KeyS, true),
        (158, KeyS, false),
        (166, KeyW, true),
        (167, KeyW, false),
        (178, Enter, true),
        (179, Enter, false),
        (195, KeyA, true),
        (214, KeyA, false),
    ];
    assert!(sim.start_recording());
    for step in 0..WARMUP + MEASURED {
        sim.queue_input(inputs.iter().filter(|(at, _, _)| *at == step).map(
            |&(_, key, pressed)| InputEvent {
                window,
                timestamp: InputTimestamp::Received(STEP * step as u32),
                input: Input::Key {
                    key: PhysicalKey::Code(key),
                    pressed,
                    repeat: false,
                },
            },
        ));
        assert_eq!(sim.advance(STEP), 1);
    }
    sim.finish_recording().expect("reference input recording")
}
