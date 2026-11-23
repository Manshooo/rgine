# ADR 0015 — Render graph compilation and execution

- Status: Accepted
- Date: 2026-11-24
- Tracking issue: [#39](https://github.com/Manshooo/rgine/issues/39)

## Context

ADR 0014 gives the runtime a live drawable. Scene rendering also needs to order producers such as the shadow depth pass before their consumers and to identify resources that contribute to a frame.

## Decision

- Declare imported resources and transients with opaque handles owned by a graph. An import may be read without a writer. A transient must have a producer before any read or export.
- Each handle describes one logical version of a resource and has at most one writer. A pass declares its reads and writes before compilation. A new version gets a new handle. A pass that reads and writes the same version creates a dependency on itself.
- Compile a dependency edge from each producer to its readers. When multiple passes are ready, prefer their declaration order. Compilation rejects foreign handles, missing and conflicting producers, and cycles. Cycle diagnostics report a closed path of pass names.
- Export frame results explicitly. The backend walks the compiled pass order with the resource bindings for the current frame. The window renderer acquires and binds the current texture, encodes the clear pass through the graph, then submits and presents the exported backbuffer.
- Retain the same compiled graph across redraws and changes in drawable size. wgpu objects and resource bindings stay in `engine-render`.

## Consequences

Passes declare their dependencies independently of registration order. The initial executor runs all declared passes. The graph's exports and access declarations provide the inputs for pass culling and transient allocation reuse, tracked by [#40](https://github.com/Manshooo/rgine/issues/40) and [#41](https://github.com/Manshooo/rgine/issues/41).
