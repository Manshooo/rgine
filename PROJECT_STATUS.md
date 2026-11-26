# Project status
**Date**: 2026-11-26
**Milestone**: 4 - Physics + Game UI
**Version**: 0.1.0-dev

## Phase 0 exit
Both criteria hold:

- an empty window runs - `cargo run -p engine-app --example empty_window` opens a window and closing it returns from `run`;
- `cargo check --workspace --locked` and `cargo test --workspace --locked` pass, on top of fmt, clippy, the MSRV toolchain, `cargo deny check all` and `cargo xtask check-deps`, all required by the branch ruleset.

The cross-cutting iteration budget criterion in ROADMAP is not enforced yet; see Deferred.

## Done
- [x] Cargo workspace skeleton
- [x] Domain/app crate boundaries
- [x] Rust toolchain policy and a CI job for the minimum supported Rust version
- [x] Dual MIT OR Apache-2.0 licensing
- [x] Repository name and canonical URL: `rgine`, https://github.com/Manshooo/rgine
- [x] ADR structure
- [x] CI for desktop, the Android target, MSRV, dependency policy and boundaries
- [x] Minimal core time/logging
- [x] Minimal winit bootstrap
- [x] CLI bootstrap
- [x] ADR 0003 type registry and reflection
- [x] ADR 0004 authoring data model
- [x] ADR 0005 compatibility and versioning policy
- [x] ADR 0006 no code hot reloading in 0.1
- [x] ADR 0007 gameplay scripting layer (Proposed - deferred, no scripting in 0.1)
- [x] ADR 0009 platform is a replaceable backend; `app` no longer depends on `winit`
- [x] `cargo xtask check-deps`: editor/devtools unreachable from runtime crates, backend crates named only by the crate that owns them (ADR 0009)
- [x] `cargo deny` with a permissive-only licence allow-list, fatal advisories and crates.io as the only source
- [x] Iteration budgets defined in ROADMAP
- [x] An empty window that opens and closes cleanly

## Phase 1 exit

- The fixed-update benchmark measures 1,000 and 100,000 entities in CI. On the local Windows run, the median at 1,000 entities was 1.81 µs per update.
- The deterministic replay test records and serializes input, then verifies that replay produces the same world state. The `engine-app` and `engine-ecs` test suites passed locally.
- The type registry and reflection derive, archetype and sparse storage, filtered queries, change detection, deferred commands, relationships, required components and hooks are in `master`. The app runs the deterministic ECS schedule with fixed steps, plugins and input recording.
- [Issue #23](https://github.com/Manshooo/rgine/issues/23) tracks native OS timestamps as follow-up work. The current input adapter supplies monotonic receipt timestamps with explicit provenance.

## Phase 2 exit

Phase 2 uses wgpu 30.0.2 with minimum Rust 1.87. The [surface spike](https://github.com/Manshooo/rgine/issues/37) exercises the platform contract in [ADR 0014](docs/adr/0014-render-backend-and-surface-lifetime.md). The app presents a clear color through the regular window loop ([#38](https://github.com/Manshooo/rgine/issues/38)), and the clear pass now executes through the compiled render graph ([#39](https://github.com/Manshooo/rgine/issues/39), [ADR 0015](docs/adr/0015-render-graph.md)). The [quick start](README.md#первый-запуск) shows how to run the example and the input replay test.

The graph culls unused passes and reuses transient allocations ([#40](https://github.com/Manshooo/rgine/issues/40), [#41](https://github.com/Manshooo/rgine/issues/41), [ADR 0016](docs/adr/0016-transient-render-graph-allocations.md)). The resource table supports vertex, index and uniform buffers, sampled textures and samplers, and adapter-selected classic or draw-uniform bindless bindings ([#42](https://github.com/Manshooo/rgine/issues/42), [#55](https://github.com/Manshooo/rgine/issues/55), [#44](https://github.com/Manshooo/rgine/issues/44), [ADR 0017](docs/adr/0017-render-resource-table.md)).

The procedural ECS scene renders opaque metallic-roughness materials, directional shadows and a skybox ([#43](https://github.com/Manshooo/rgine/issues/43), [#46](https://github.com/Manshooo/rgine/issues/46), [#47](https://github.com/Manshooo/rgine/issues/47), [#48](https://github.com/Manshooo/rgine/issues/48)). The build caps shader variants, CI validates the pipeline catalogue, and the renderer prepares the applicable pipelines before first use ([#45](https://github.com/Manshooo/rgine/issues/45), [#49](https://github.com/Manshooo/rgine/issues/49)).

The [exit run](https://github.com/Manshooo/rgine/issues/51) passed on the approved Windows machine with a Core i7-9850H and Intel UHD Graphics 630. With DX12, classic bindings and a 1280 × 720 SDR drawable in Immediate mode, the 20-second orbit delivered 13,260 frames at 662.84 presents/s, with p99 at 3.05 ms and a maximum interval of 19.47 ms. Three graphics pipelines were prepared before the 120-frame warmup. The warmup and orbit each created zero pipelines. The [measurement protocol](docs/render-v1-measurement.md) defines the workload and thresholds and records the originally specified RTX 3060 configuration.

The Linux CI host job enforces the 10-second incremental gameplay build budget using the procedural scene example. Deferred rendering work is tracked in [Render follow-ups](https://github.com/Manshooo/rgine/milestone/9).

## Phase 3 exit

The [acceptance report](docs/phase-3-exit.md) records the save and load round-trip, three-way RON merge and hot reload in the running scene. The measured import, asset reload, shader reload and incremental build stayed within their budgets. The scripting review kept gameplay in Rust for 0.1, with tuning and transitions exposed as data.

## Next

[Phase 4](https://github.com/Manshooo/rgine/milestone/5) starts with [input actions](https://github.com/Manshooo/rgine/issues/68), [fixed-step physics](https://github.com/Manshooo/rgine/issues/85) and [XML widget trees](https://github.com/Manshooo/rgine/issues/88). Input and physics unblock game-owned movement. The widget tree unblocks CSS and layout. The phase verifies gameplay, input and UI on desktop and Android. A separate game can demonstrate these features after the phase.

## Deferred
- [ ] CI checks the incremental gameplay build budget. The remaining iteration budgets need editor and asset workflows before CI can measure them.
- [ ] `engine-cli new` creating a project from `templates/`. Not required by the phase 0 exit criterion; it belongs with the launcher and packaging work in phase 6, or earlier if the templates are needed to test the asset pipeline.
- [ ] Public API and schema snapshot checks (ADR 0005), due with phase 7.
