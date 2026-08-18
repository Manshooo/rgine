# ADR 0001: Workspace and ECS ownership

- Status: Accepted
- Date: 2026-08-18

## Decision
Use a Cargo workspace with independently testable crates and implement the engine ECS in-house.

## Reasons
- strict modularity;
- control over ECS semantics and deterministic iteration;
- no dependence on a third-party engine ECS release cycle;
- clear runtime/editor boundary;
- incremental compilation.

## Consequences
The ECS becomes a long-term maintenance responsibility. The first implementation therefore stays intentionally small: archetypes, queries, deferred commands, stages, change detection, and deterministic iteration hooks.
