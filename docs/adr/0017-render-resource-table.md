# ADR 0017 — Render resource table

- Status: Proposed
- Date: 2026-11-24
- Tracking issue: [#42](https://github.com/Manshooo/rgine/issues/42)

## Context

The first 3D scene needs to refer to mesh and shader resources from extracted ECS data. ADR 0014 keeps wgpu inside the renderer, and the roadmap requires a classic bind-group path that works on adapters without binding arrays. The bindless path follows the scene integration. [First 3D scene #43](https://github.com/Manshooo/rgine/issues/43), [bindless tables #44](https://github.com/Manshooo/rgine/issues/44).

## Decision

### Ownership and handles

- Keep one table per render device. It owns backend objects for buffers, textures and samplers and gives callers typed, opaque handles. Each handle identifies its table, slot and generation. The table checks all three when resolving the handle.
- Use renderer-owned descriptors and handles in the scene interface. The render graph can import table resources for a frame. Graph-local transient allocations belong to the graph's allocation plan.
- Release advances the slot generation. Retire the slot if the generation cannot advance. Recreating the device creates a new table identity, so handles from the old device are rejected.

### Updates and bind groups

- Apply resource mutations before encoding a frame. Validate handle ownership, generation, descriptor compatibility and upload ranges before applying each operation. Failed validation leaves the existing binding intact.
- Content uploads preserve the handle and existing bind groups. Replacing the backing object also preserves the handle when the replacement satisfies the same binding contract. Changes to the binding contract require a new handle.
- The classic backend builds bind groups for a pipeline layout and its ordered entries, including view and range information. Replacing or releasing a backing object invalidates affected cached groups before later frames use them. Groups already referenced by submitted work keep their normal GPU lifetime.

### Backend selection

- #42 initializes the classic backend on baseline device features. Scene handles have the same resolution and update semantics across binding backends.
- #44 selects bindless before creating the device when the adapter meets every feature and limit required by its layouts. It uses classic otherwise and provides a debug override to exercise classic on a bindless-capable adapter. The selected layout is part of the pipeline identity and remains fixed for the device.

## Verification

- Exercise allocation, content upload, compatible replacement, release and slot reuse. Verify that a stale handle and a handle from another table both fail resolution.
- Exercise a classic bind group on a device requested without optional binding-array features. Verify that replacing a resource changes subsequent bindings and that content updates preserve them.
- #43 consumes the resource handles in the procedural scene. #44 compares the classic and bindless paths on a compatible adapter.
