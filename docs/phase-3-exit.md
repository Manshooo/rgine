# Phase 3 acceptance report

Verified on 2026-11-26 using Windows 11 build 26300 (x86-64), Rust 1.100.0 and DirectX 12 for the visible-frame measurements.

## Authored scene to live world

```sh
cargo test --locked -p engine-app --features hot-reload --lib hot_reload::phase3::assets_and_scenes_phase_exit -- --exact
```

The fixture imports `textured-quad.gltf` with a KTX2 base-color map and saves and reopens a prefab and a scene linked to it. It resolves the prefab into the RON snapshot watched by the running world's `PropScene`. Editing the glTF in place moves all three render instances from z = −2 to z = −3 and replaces the active mesh.

From one base scene, one author overrides the prefab child's translation and another changes the independent placement. The production three-way RON merge combines the edits without manual resolution. The same world polls the resolved merge result and contains the expected y positions of −2, 0 and 2. It retains all three entity handles, the prefab child's parent and the reloaded textured mesh.

The acceptance commands run locally. Platform CI builds the workspace and examples.

## Measurements

| Measurement | Median | Samples | Budget |
| --- | ---: | --- | ---: |
| Warm glTF import of 10,000 triangles | 17.9 ms | 5 cache hits | < 100 ms |
| External glTF buffer edit to visible frame | 70.1 ms | 70.1, 67.7, 79.6 ms | < 2 s |
| RON placement edit to visible frame | 251.2 ms | 389.2, 251.2, 248.7 ms | < 2 s |
| WGSL edit to visible frame | 349.2 ms | 348.2, 349.5, 349.2 ms | < 1 s |
| Incremental gameplay example build | 6.495 s | 5.737, 6.495, 6.871 s | < 10 s |

The first-frame fixture samples the game's window while the same process runs. It checks that a truncated glTF buffer, malformed RON and invalid WGSL preserve the previous visible result. Corrected edits change the marker's position or color. See the [Windows and Linux commands](CONTRIBUTING.md#reload-an-authored-prop-during-development).

```powershell
cargo test --locked -p engine-asset --test gltf_prop measures_warm_reimport_of_ten_thousand_triangles -- --exact --nocapture
$env:CARGO_INCREMENTAL = '1'
python .github/scripts/check-incremental-build.py
```

## Gameplay scripting review

Reviewed the three triggers in [ADR 0007](adr/0007-gameplay-scripting.md#trigger-conditions).

1. **Incremental build budget.** The measured example stays under 10 s. It represents the current gameplay fixture. The decision would require a realistic project exceeding the budget after the ADR's mitigations.
2. **A non-Rust gameplay author.** The Phase 4 scope identifies gameplay authored in Rust, with tuning, curves and transitions exposed as RON data. No requirement for a non-Rust author to implement gameplay behavior was identified in the phase scope.
3. **Third-party logic.** The current game scope has no requirement to execute third-party gameplay logic. The roadmap tracks mod loading separately after the MVP.

The Phase 3 review leaves the scripting decision deferred for 0.1.
