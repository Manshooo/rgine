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


Editor-only functionality must never become a runtime dependency.

## Two data levels

Authoring data and runtime data are separate representations with a deterministic compilation between them (ADR 0004).

`Truth` holds authoring data: objects addressed by GUID, mutated only through a change journal, serialized to RON, schema described by the type registry. The editor, CLI and launcher operate on `Truth` exclusively.

`World` holds runtime data: archetype storage, queries, deferred commands, stages.

`Truth -> World` compilation is incremental. Development builds keep a bidirectional `Entity` to `AuthoringId` map; release builds strip it and do not link `Truth` at all.

## Backend API rules

The rules below apply to the public surface of `render`, `physics`, `audio` and `script`. They are what makes those backends genuinely replaceable rather than nominally so.

- **Handles, not pointers.** Callers hold opaque identifiers; the backend owns the storage.
- **Commands in one direction.** Backends may process work asynchronously. Any call that returns a value is therefore a synchronization point, and the API is shaped so such a call is hard to write by accident in a per-frame path.
- **Batched entry points.** Public functions operate on slices, not on single items: `update_transforms(&[Handle], &[Mat4])` rather than one call per object.
- **No backend type in a caller signature.** If a caller names a type owned by a backend, the abstraction has already leaked.

## Enforced boundaries

These are checked by `xtask` in CI, not stated as convention:

- `editor` and `devtools` are not reachable from any runtime crate's dependency graph;
- domain crates do not depend on `apps`;
- the public API surface and serialized schemas match their recorded snapshots, per ADR 0005.
