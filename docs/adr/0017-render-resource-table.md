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

### Phase 2 scope

- #42 implements vertex, index and uniform buffer entries and the classic uniform bind groups required by the first scene.
- The contract also covers sampled texture views and samplers. A texture view declares its binding type and subresource range. A sampler declares the filtering or comparison behavior required by its binding. The handles follow the same ownership, replacement and release rules as buffers. Native allocation and classic bindings for these entries belong to [#55](https://github.com/Manshooo/rgine/issues/55).

### Updates and bind groups

- Submit the previous frame's encoded commands before applying mutations for the next frame. Queue uploads before encoding and submitting that next frame. GPU execution can overlap CPU frame preparation, with submission order preserving the contents each frame reads.
- Validate handle ownership, generation, the declared binding contract and upload ranges before applying each operation. Content uploads preserve the handle and existing bind groups.
- Publish a replacement only after validation succeeds. It must satisfy the declared binding contract and keep all existing binding ranges and views valid. For example, a buffer replacement must retain the required usages and cover every existing bound range. On failure, the current backing and bind groups stay valid. Changes to the declared binding contract require a new handle.
- The classic backend builds bind groups from a bind-group layout and its ordered entries, including view and range information. Compatible replacements preserve the handle. Replacements and releases invalidate the affected cached groups for subsequent frames. Previously submitted work keeps its existing bindings and their backend objects for its GPU lifetime.

### Backend selection

- #42 initializes the classic backend on baseline device features. Scene handles have the same resolution and update semantics across binding backends.
- #44 selects the binding mode before creating the device. Select bindless when the adapter meets every feature and limit required by the supported bindless profile. Select classic when any requirement is missing or a debug override forces classic. The binding mode remains fixed for the device. Each pipeline has its own layout, and its pipeline key includes the binding mode and the layout signature.

## Verification

- Exercise allocation, content upload, compatible replacement, release and slot reuse. Verify that a stale handle and a handle from another table both fail resolution.
- Exercise a classic uniform bind group on a device requested without optional binding-array features. Verify that content updates preserve it, a compatible replacement changes subsequent bindings, and a smaller buffer that would invalidate a bound range leaves the existing binding usable.
- #43 consumes the resource handles in the procedural scene. #44 compares the classic and bindless paths on a compatible adapter.
