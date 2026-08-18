# Roadmap

## Phase 0. Foundation
Workspace, tooling, CI, licenses, ADR process, core/platform skeleton.

**Exit**: пустое окно запускается, workspace проходит check/test.

## Phase 1. ECS + App Loop
Archetype storage, World/Query/Commands, stages, change detection, fixed timestep, Plugin API.

**Exit**: 1000 entities update benchmark + deterministic test.

## Phase 2. Render v1
wgpu device/surface, Render Graph, camera, PBR, shadows, skybox.

Exit: primitive scene at stable 60 FPS on reference desktop.

## Phase 3. Assets + Scenes
GUID, .meta, import cache, glTF, KTX2, RON, migrations, prefabs, hot reload.

**Exit**: save/load round-trip and live scene reload.

## Phase 4. Physics + 3D Platformer
Rapier adapter, character controller, interpolation, checkpoints, coins, mobile controls.

**Exit**: playable prototype on desktop + Android.

## Phase 5. Editor
Hierarchy, Inspector, Viewport, gizmos, selection, Asset Browser, Console, Undo/Redo, isolated Play World.

**Exit**: level can be built/saved/reopened/played without editor logic leaking into runtime.

## Phase 6. Launcher + Packaging + Audio
Project manager, engine version selection, package formats, .pak, Kira adapter.

**Exit**: one command produces runnable packages for target platforms.

## Phase 7. Stabilization / 0.1.0
Performance, diagnostics, docs, release automation, regression game.

**Exit**: quickstart from empty machine to playable project in <= 15 minutes.

Non-MVP backlog
Animation, navigation, advanced audio, material graph, terrain, networking, iOS, consoles, marketplace.
