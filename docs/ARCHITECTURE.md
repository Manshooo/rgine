# Architecture
Dependency direction:

`apps -> app -> domain crates`

Domain crates must not depend on `apps`.

Core layers:

**1.** `core` - foundational types, time, jobs, logging, type registry and reflection (ADR 0003).

**2.** `ecs` - data-oriented world and scheduler.

**3.** `platform` - OS/window/input abstraction.

**4.** `render`, `physics`, `audio` - replaceable backends. `script` is a reserved slot of the same shape, empty for 0.1 (ADR 0007).

**5.** `asset`, `scene` - data/runtime services.

**6.** `app` - integration, plugins, lifecycle.

**7.** `engine-cli`, `editor`, `launcher` - clients of the public engine API.


Derive macros live in a proc-macro crate owned by the crate whose trait they implement - `core-macros`, `ecs-macros` - re-exported under the name of the owner and named by nobody else (ADR 0011). Generated code names the owner crate only, so an expansion never crosses an edge the dependency graph does not show.

Editor-only functionality must never become a runtime dependency.

## Two data levels

Authoring data and runtime data are separate representations with a deterministic compilation between them (ADR 0004).

`Truth` holds authoring data: objects addressed by GUID, mutated only through a change journal, serialized to RON, schema described by the type registry. The editor, CLI and launcher operate on `Truth` exclusively.

`World` holds runtime data: archetype storage, queries, deferred commands, stages.

The full `Truth -> World` compiler is the reference for incremental compilation. Development builds keep a bidirectional `Entity` to `ObjectId` map. Shipped games disable `engine-scene`'s `authoring` feature and load compiled data without that map or Truth. [ADR 0019](adr/0019-compiled-scene-runtime.md) records the compiled artifact and instantiation contract.

`IncrementalScene` reconciles resolved Truth snapshots into a live development world and preserves surviving `Entity` handles. [ADR 0022](adr/0022-incremental-scene-compilation.md) defines the affected nodes and how updates are prepared and applied.

## Backend API rules

The rules below apply to the public surface of `platform`, `render`, `physics`, `audio` and `script`. They are what makes those backends genuinely replaceable rather than nominally so. `platform` is covered on the same terms as the rest (ADR 0009): windowing is a backend choice like any other.

- **Handles, not pointers.** Callers hold opaque identifiers; the backend owns the storage.
- **Commands in one direction.** Backends may process work asynchronously. Any call that returns a value is therefore a synchronization point, and the API is shaped so such a call is hard to write by accident in a per-frame path.
- **Batched entry points.** Public functions operate on slices, not on single items: `update_transforms(&[Handle], &[Mat4])` rather than one call per object.
- **No backend type in a caller signature.** If a caller names a type owned by a backend, the abstraction has already leaked.

## Shader variants

Render v1 counts each registered WGSL program and binding mode combination as
one shader variant. The procedural opaque program has two variants in
declaration order, classic and draw-uniform bindless. Solid colors and
checkerboards use the same programs with different draw data. Attachment
formats and layout signatures belong to pipeline keys.

The renderer's internal `SHADER_VARIANTS` list enumerates the supported keys
for shader creation and pipeline catalogue validation. Register each supported
combination explicitly so a new axis only contributes valid combinations. The
global cap is eight variants and the renderer fails to compile if the list
exceeds it. The shader tests print the registered count and keys and check that the budget
rejects a fixture one past the cap.

The catalogue covers the scene's six drawable color formats and both sampled
binding profiles. The directional shadow key is color independent, and skybox
keys are shared by both binding profiles. The renderer tests create these pipelines with the
runtime layouts and descriptors on the test-only wgpu Noop backend, which runs
wgpu validation without a native graphics driver. Drawables prepare the keys for
their selected configuration and retain them for the lifetime of the device.
The renderer logs the preparation time and exposes a cumulative creation count
for sampling around a measured pass.

## Enforced boundaries

These are checked by `cargo xtask check-deps` in CI, not stated as convention:

- `editor` and `devtools` are not reachable from any runtime crate's dependency graph;
- domain crates do not depend on `apps`;
- a backend crate is named only by its owner. `engine-platform` owns `winit`, `engine-render` owns `wgpu`, and `engine-physics` owns `rapier3d` (ADR 0009, ADR 0014 and ADR 0026);
- a macro crate (`engine-core-macros` or `engine-ecs-macros`) is named only by the crate that owns it, per ADR 0011;
- the public API surface and serialized schemas match their recorded snapshots, per ADR 0005 (not yet implemented; due with Phase 7).
