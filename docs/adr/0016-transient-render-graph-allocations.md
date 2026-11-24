# ADR 0016 — Transient render graph allocations

- Status: Accepted
- Date: 2026-11-24
- Tracking issue: [#41](https://github.com/Manshooo/rgine/issues/41)

## Context

ADR 0015 defines logical resource versions and ADR 0014 keeps GPU objects inside the renderer. A logical version can reuse the physical storage of an earlier version when their GPU accesses are ordered and their storage requirements match. The retained pass order from #40 determines which accesses matter.

## Decision

- Compute inclusive lifetimes over retained reads and writes. A resource accessed by a culled pass gains no use from that pass. An export counts as a use immediately after execution.
- Give each physically backed transient a descriptor. The renderer defines descriptor equality using every field relevant to storage compatibility. A logical transient without a descriptor participates in dependency ordering and has no planned allocation.
- Process transients by first use, breaking ties by declaration order. Reuse a slot with an equal descriptor only when its previous last use is strictly earlier than the new first use. Passes that read one version and write another in the same step require separate storage.
- Import and persistent bindings are supplied by the renderer for each logical handle and stay outside the transient allocator. A persistent binding can supply initial contents across graph executions.
- Allocate one backend object per slot. The renderer resolves transient handles through the compiled plan and can retain the objects across executions with the same descriptors. Producers initialize the contents they write, and passes use each logical resource only within its declared accesses.

## Consequences

The compiler can report how many allocations a frame needs without a GPU. A graph that exports a transient reserves its backing storage through the last pass, so later passes cannot overwrite the exported contents.
