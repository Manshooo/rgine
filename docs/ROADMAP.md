# Roadmap

Each phase measures its iteration budgets on the reference machine and checks them for regressions.

## Iteration budgets

Iteration time is the primary product metric of the engine, not a side effect of it. Unmeasured, it degrades silently - this is how both Unity and Unreal arrived at their current numbers. The thresholds matter less than the fact that a threshold exists and fails the build.

| Action | Budget |
| --- | --- |
| Inspector edit reflected in the viewport | < 16 ms |
| Shader edit recompiled and visible | < 1 s |
| Asset changed on disk, hot reloaded in a running game | < 2 s |
| Enter play mode from the editor | < 2 s |
| Incremental rebuild of the gameplay crate | < 10 s |
| Cold editor start on an empty project | < 3 s |
| Re-import of a typical glTF asset, cache hit | < 100 ms |

## Phase 0. Foundation
Workspace, tooling, CI, licenses, ADR process, core/platform skeleton.

**Exit**: пустое окно запускается, workspace проходит check/test.

## Phase 1. ECS + App Loop
Archetype storage, World/Query/Commands, stages, change detection, fixed timestep, Plugin API.

Also in this phase, because retrofitting each of these is a breaking migration:

- type registry and reflection derive (ADR 0003) - required before the first serialized type exists;
- sparse storage for marker and frequently toggled components, selected per component type;
- entity relationships as a first-class concept, with the parent/child hierarchy built on top of them;
- required components, replacing ad-hoc bundles;
- component add/remove hooks, so render and physics register objects without a separate synchronization system;
- deterministic input record and replay, on top of the deterministic system order.

**Exit**: 1000 entities update benchmark + deterministic test. Recorded input replays to an identical world state.

## Phase 2. Render v1
wgpu device/surface, Render Graph, camera, PBR, shadows, skybox.

- render graph with transient resource aliasing and dead pass culling;
- an explicit shader variation budget: the axes of variation are enumerated, the combination count is capped, and exceeding the cap fails the build;
- full PSO collection in CI, so first-use compilation cannot reach a shipped build;
- a resource table abstraction with two implementations - bindless and classic bind groups - selected by adapter features, because bindless coverage in wgpu is uneven across backends.

Exit: primitive scene at stable 60 FPS on reference desktop, with no first-use pipeline hitch in a scripted camera pass.

The [Render v1 measurement protocol](render-v1-measurement.md) defines the reference run and the iteration budget.

## Phase 3. Assets + Scenes
GUID, .meta, import cache, glTF, KTX2, RON, migrations, prefabs, hot reload.

- import is a pure function; the cache key is a hash of (input bytes, import settings, importer version), which makes the cache shareable between machines and CI;
- stable node-local identifiers inside a scene, without which a text format is not actually mergeable;
- a merge driver shipped with the engine and exercised by a test;
- `Truth` and the change journal (ADR 0004), with `Truth -> World` compilation.

**Exit**: save/load round-trip, live scene reload, and a three-way merge of two divergent edits to one scene resolved without manual fixing.

The [Phase 3 acceptance report](phase-3-exit.md) records the local scenario, reload times and scripting review.

## Phase 4. Physics + Game UI
Rapier adapter, game-owned movement, interpolation, checkpoints, collectibles, mobile controls, game UI.

- data-driven behaviour: configs, curves, state machines and tuning values in RON with hot reload. This is the deliverable, not a script API;
- no scripting layer and no visual scripting (ADR 0007). Reassess against the trigger conditions recorded there at the end of Phase 3.

**Exit**: a running fixture on desktop and Android verifies fixed-step gameplay, mobile input, a live collectible counter and a pause menu. Gameplay data, UI layouts and styles can be reloaded without rebuilding or restarting the fixture.

### Game UI

2D support covers the screen-space game interface. `engine-ui` keeps a tree of widgets built from XML layouts and styled with a documented subset of CSS. The authoring workflow follows [Valve Panorama](https://www.dota2.com/reborn/part2).

The [XML layout guide](game-ui-layout.md) documents widget declarations and panel reuse. The [stylesheet guide](game-ui-styles.md) defines selectors, cascade and computed style values.

- Layouts define reusable panels, labels, images and buttons. Styles define spacing, sizing, row and column layout, alignment, typography and interaction states. Layout adapts to the viewport and display scale.
- UI widgets bind to data published by the game. Named actions from the layout are dispatched as typed Rust events. For example, the HUD reads the coin count, and the pause menu emits a resume action.
- The game renders the UI over the 3D scene. UI handles pointer and touch input and keyboard focus. Input handled by a widget is kept out of gameplay controls.
- Layouts and styles are assets that reload during development. A failed reload reports the source error and keeps the last valid UI active.

### Frame profiler

Add an opt-in frame profiler at the end of Phase 4. Use Deflection Arena to validate it and reuse the capture format for the later [iteration budgets](https://github.com/Manshooo/rgine/issues/108) and [Arcanists two-colony measurements](https://github.com/Manshooo/rgine/issues/148).

1. Record CPU timings for input, fixed steps (including physics and catch-up), scene extraction, render preparation, buffer uploads, command encoding, queue submit, surface acquisition and present. Include redraw requests, present outcomes and intervals between successful presents so pacing can be diagnosed alongside workload.
2. Record GPU time per render graph pass where the adapter supports timestamp queries. Start with shadows and opaque rendering. Read results asynchronously and attach them to the original frame ID. Record unavailable or dropped GPU samples explicitly. [wgpu timestamp query example](https://wgpu.rs/doc/wgpu_examples/timestamp_queries/index.html)
3. Record per-pass draw calls, instances, triangles and pipeline or bind group changes. Record uploaded bytes and newly created pipelines per frame. Include the build profile, adapter, backend, binding mode, drawable size and present mode with each capture.
4. Export a bounded capture with named CPU and GPU ranges and a summary of median, p95 and p99 frame times. Use a trace viewer to inspect spikes and correlate the ranges and counters by frame ID. Collect samples in app and render and keep trace inspection in devtools.

The initial Windows DX12 test uses a fixed Deflection Arena replay at the same resolution and settings. Compare the original HUD, the same glyph geometry batched by material and the arena with text hidden. The original debug comparison went from 26 FPS with 599 objects to 60 FPS with 34 while hiding text. The batched case holds the glyph geometry constant while reducing draw submissions and per-object setup. The capture should show where the difference appears in CPU preparation and encoding, GPU pass time and presentation pacing. Measure profiler overhead in separate disabled, CPU-only and CPU plus GPU runs.

### Signal Run v2 after Phase 4

After Phase 4 closes, rebuild [Signal Run](https://github.com/Manshooo/signal-run) as the 3D platformer that evaluates the completed engine features. Keep its seven-platform, five-coin level as the comparison route. Use fixed-step Rapier movement and interpolation, keyboard and touch input actions, and an XML/CSS HUD and pause menu with live bindings.

Run the route on desktop and Android. Verify checkpoints, respawn, pause and finish, and measure frame pacing and the time for gameplay tuning and UI edits to appear in the running game. Record the game and engine revisions, the comparison with v1 and any gaps exposed by the migration. [Signal Run v2 evaluation plan](https://github.com/Manshooo/rgine/issues/180).

## Phase 5. Editor
Hierarchy, Inspector, Viewport, gizmos, selection, Asset Browser, Console, Undo/Redo, isolated Play World.

- undo is journal rewind over `Truth`, not a Command implementation per tool;
- the inspector is generated from the type registry, not hand-written per type;
- data validation with actionable messages: an object that fails to render reports why, naming the missing GUID.

**Exit**: level can be built/saved/reopened/played without editor logic leaking into runtime, and the `xtask` dependency check proves it.

## Phase 6. Launcher + Packaging + Audio
Project manager, engine version selection, package formats, .pak, byProd audio adapter using the existing bindings.

**Exit**: one command produces runnable packages for target platforms.

## Phase 7. Stabilization / 0.1.0
Performance, diagnostics, docs, release automation, regression game.

- public API surface and schema snapshot checks in CI, enforcing ADR 0005;
- examples and templates built in CI, so documentation cannot silently rot.

**Exit**: quickstart from empty machine to playable project in <= 15 minutes.

## Scope note

Phases 0-7 amount to a custom ECS, a PBR renderer, an asset pipeline, an editor with undo and gizmos, a launcher, multi-platform packaging, audio and a sample game. For calibration: The Machinery covered comparable ground with a team of ten. Treat "one small game shipped entirely on rgine" as the success criterion rather than "the engine is done", and defer anything that game does not need.

Non-MVP backlog
Animation, navigation, advanced audio, material graph, terrain, networking, iOS, consoles, marketplace. Script debugger integration only if ADR 0007 is ever accepted.

The [Arcanists post-MVP backlog](post-mvp-arcanists.md) breaks the first two-colony prototype into testable tasks, starting with star-system simulation.

Mod loading is a separate track and mostly not a scripting question: an asset mount layer with priority and conflict resolution (useful for DLC, localization and patches regardless of mods), a mod package format and manifest, and a versioned host API surface. A wasm backend is added only if a hard capability boundary for untrusted third-party code is required; where the game already embeds a script VM, mods use that VM.
