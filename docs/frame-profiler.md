# Deflection Arena frame profiler

## Windows DX12 reference run

The debug build ran on Windows 11 with a Ryzen 7 5700X and a GeForce RTX 3060 (driver 32.0.16.2140). Each run used DX12, classic bindings, a 1120 × 720 drawable, BGRA8 sRGB, and Mailbox presentation. The same input recording replayed one fixed step per successful present, with 30 warmup presents followed by 240 measured presents. The original HUD draws individual glyph cells as 3D objects, the batched HUD combines the same geometry by material, and the hidden HUD removes that text.

Run from the repository root in a Visual Studio Developer PowerShell.

```powershell
$env:WGPU_BACKEND = 'dx12'
$env:RGINE_FORCE_CLASSIC = '1'
foreach ($hud in @('original', 'batched', 'hidden')) {
    foreach ($capture in @('off', 'on', 'gpu')) {
        $name = "${hud}-${capture}"
        cargo run --locked -p engine-app --features physics --example deflection_arena -- --profile $hud $capture "$name.json"
        cargo run --locked -p engine-devtools --bin frame_capture -- "$name.json" "$name.trace.json" "$name.summary.json"
    }
}
```

### CPU work and presentation

Present intervals are measured between successive returns from the application's present path. CPU ranges in the trace measure work inside each frame attempt. Every CPU capture recorded 240 successful attempts and zero omitted attempts or dropped spans. Perfetto v59.1 imported the original HUD trace with 240 frame attempts and 240 command encoding slices. [Perfetto Chrome JSON import](https://perfetto.dev/docs/getting-started/other-formats#chrome-json-format)

| HUD | Capture off present interval, median / p95 / p99 (ms) | CPU capture present interval, median / p95 / p99 (ms) |
|---|---:|---:|
| Original | 25.015 / 40.805 / 42.776 | 24.862 / 40.508 / 44.849 |
| Batched | 3.353 / 4.243 / 5.375 | 3.398 / 4.574 / 5.652 |
| Hidden | 3.719 / 4.969 / 5.449 | 2.944 / 3.761 / 3.960 |

| HUD | Fixed step median (ms) | Scene extraction median (ms) | Render preparation median (ms) | Command encoding median (ms) | Mean draws per pass | Mean triangles per pass | Mean uploaded bytes per frame |
|---|---:|---:|---:|---:|---:|---:|---:|
| Original | 0.256 | 0.440 | 6.206 | 16.364 | 652.81 | 4054.05 | 334240 |
| Batched | 0.192 | 0.043 | 0.520 | 1.630 | 39.84 | 4054.05 | 20748 |
| Hidden | 0.166 | 0.037 | 0.449 | 1.337 | 34.27 | 2816.97 | 17547 |

The directional shadow and opaque passes each submitted the same geometry. The original and batched workloads matched triangle counts in every corresponding frame. Batching reduced render preparation, encoding, and upload work while preserving those counts. The observed CPU-capture median interval changes were −0.153 ms, +0.045 ms, and −0.775 ms for the original, batched, and hidden HUDs. Each cell is one replay run, so run-to-run variation is part of those differences.

### GPU pass timings

The `gpu` mode adds timestamp queries to CPU capture. It uses four readback slots and collects completed samples by the ID of the frame that issued each query. An unsupported adapter reports `unavailable` and retains the CPU ranges and render counters. A full readback ring reports `dropped`. A sample that fails or stays pending at the end of the capture is `incomplete`.

| HUD | CPU + GPU present interval, median / p95 / p99 (ms) | Median change from CPU-only (ms) | Directional shadow GPU, median / p95 / p99 (ms) | Opaque scene GPU, median / p95 / p99 (ms) |
|---|---:|---:|---:|---:|
| Original | 25.575 / 43.672 / 46.042 | +0.713 | 0.043 / 0.054 / 0.055 | 0.058 / 0.071 / 0.072 |
| Batched | 3.524 / 4.917 / 6.096 | +0.126 | 0.029 / 0.033 / 0.033 | 0.040 / 0.043 / 0.044 |
| Hidden | 3.184 / 4.129 / 4.741 | +0.240 | 0.027 / 0.030 / 0.030 | 0.039 / 0.042 / 0.043 |

Each GPU run returned 240 complete results. Perfetto imported 240 slices for each GPU pass in the original run. On this debug fixture, the largest measured change from batching was in CPU render preparation and command encoding. The offscreen canvas test on DX12 verifies a `screen-space canvas` timestamp when the pass runs.

The inspector places each GPU group at the CPU start of its associated frame and preserves the GPU offsets within the group. Use `frame_id` to compare work across clocks. Timestamp differences are converted to nanoseconds with the submitting queue's timestamp period. [wgpu timestamp query example](https://wgpu.rs/doc/wgpu_examples/timestamp_queries/index.html)
