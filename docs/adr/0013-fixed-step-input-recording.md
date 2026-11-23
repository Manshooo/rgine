# ADR 0013: Fixed-step input recording

- Status: Accepted
- Date: 2026-11-23

## Decision

An input recording is a versioned runtime diagnostic. Version 1 records the fixed step duration, the initial completed step index, and an ordered list of input batches, including empty steps. The application assigns queued input to the next fixed step that runs. Each batch is visible to all systems in that step. Receipt and native timestamps are kept as event metadata.

Replay runs the same schedule once per recorded step. It validates the version, duration and starting step before advancing the simulation. The caller is responsible for rebuilding the same initial world, resources and systems. A recording contains input rather than a world snapshot, and replay consumes neither wall-clock time nor pending live input.

The format is represented by Serde types. The replay test exercises a JSON round trip. Authoring metadata continues to use the project data format from ADR 0002.
