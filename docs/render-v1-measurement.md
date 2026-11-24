# Render v1 measurement protocol

[Issue #51](https://github.com/Manshooo/rgine/issues/51) will implement the scripted run and record the phase exit measurements.

## Reference configuration

| Setting | Value |
| --- | --- |
| CPU | AMD Ryzen 7 5700X |
| GPU | NVIDIA GeForce RTX 3060, 12 GiB |
| OS and driver | Windows 11, build 26300.10075; NVIDIA 621.24 |
| Graphics API and resources | Direct3D 12, classic bind groups |
| Drawable | Windowed, 1280 × 720 physical pixels, SDR |
| Presentation | Immediate, with continuous redraws and no application frame cap |

Keep the window visible and at the fixed drawable size for the full run. The harness must verify the adapter, API, binding mode, drawable size and presentation mode before sampling. An unavailable or different configuration invalidates the reference run.

Build before measuring. From PowerShell in the workspace root, use the following entry point once #51 adds `--render-v1-benchmark` to the procedural scene example:

```powershell
cargo build --release --locked -p engine-app --example procedural_scene
$env:WGPU_BACKEND = 'dx12'
$env:RGINE_FORCE_CLASSIC = '1'
$env:RUST_LOG = 'info'
.\target\release\examples\procedural_scene.exe --render-v1-benchmark
```

## Scene and camera pass

Use the same procedural scene for every run. Include an opaque ground plane and occluding primitives that show smooth and rough dielectric and metallic materials, a directional light with a visible cast shadow, and a skybox. Keep the entities, material parameters, light, shadow-map size, surface format and sample count fixed throughout the run. Use a 60° vertical field of view and near and far planes of 0.1 m and 100 m.

1. Start a fresh process with an empty application pipeline cache. Leave the OS and driver caches in their normal state and record that state. Prepare all pipeline keys applicable to the reference configuration before the first frame.
2. Warm up for 120 successfully submitted and presented frames at the starting pose. Abort the run if any draw is retried, skipped or fails, or if the surface or device is reconfigured after the first warmup frame. Check that the pipeline creation counter stays unchanged from the end of preparation onward, including warmup.
3. After warmup, orbit the camera once over 20 wall-clock seconds. Set `u = min(elapsed_seconds / 20, 1)`, in the range `[0, 1]`. Look at `(0, 0.6, 0)` with +Y up, and use azimuth `2πu`, elevation `0.35 + 0.2 sin(2πu)` radians, and radius `6.2 + 1.2 sin(2πu)` metres. At azimuth zero the camera sits on the +Z side of the target. Render every frame from the current elapsed time. End after presenting the first frame at `u = 1`.

## Pass criteria and evidence

Timestamp each successful present call at return with a monotonic clock. Use the last warmup present as the initial timestamp so the first measured frame contributes its full interval. Stop sampling after the final orbit frame and wait for submitted GPU work once. Report the drain time separately and include it in the elapsed time used for throughput. These measurements describe application frame delivery.

The run passes when all of the following hold:

- Throughput is at least 60 frames per second, calculated from the measured frame count and elapsed time including the final GPU drain.
- The 99th percentile interval between presents is at most `1000 / 60` ms. Use the nearest rank `ceil(0.99 × sample_count)` in the sorted intervals. The maximum interval must be at most `2000 / 60` ms, including the first frame and the first draw of each pipeline key.
- The measured frames draw opaque PBR materials, directional shadows and the skybox.
- Zero graphics pipelines are created after preparation. Count every renderer pipeline creation at the factory, including those triggered during warmup or on the first measured frame.

Record the commit, `rustc -Vv`, GPU and driver, surface configuration, binding mode, rendered frame count, elapsed time, FPS, p50, p95, p99, max frame interval and final GPU drain time. Report pipeline preparation separately with its elapsed wall time, keys and pipeline count. Record pipeline creation counts separately for warmup and the camera pass, and record the pipeline keys exercised by the pass. Attach the raw frame samples and a reference scene screenshot to the #51 result so the measurements and workload can be checked together.

## CI iteration budgets

The existing Linux host job times a warm incremental build of the `procedural_scene` example as the Phase 2 gameplay workload. With `CARGO_INCREMENTAL=1`, the check builds the example once, changes its window title three times and measures each successive `cargo build --offline --locked -p engine-app --example procedural_scene`. It logs the samples and fails when the median reaches 10 seconds. The warm build and dependencies are outside the measured intervals. The ECS fixed-update benchmark continues to report timing separately.

The pull request CI catalogue check in [#49](https://github.com/Manshooo/rgine/issues/49) covers pipeline validation. Run the frame-rate gate on the reference desktop for [#51](https://github.com/Manshooo/rgine/issues/51). Inspector updates, live shader edits, asset reload, entering play mode, cold editor start and cached glTF imports need their corresponding editor or asset workflows before CI can measure those roadmap budgets.
