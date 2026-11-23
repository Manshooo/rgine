# ADR 0012: Deterministic ECS schedule

- Status: Accepted
- Date: 2026-11-23

## Decision

Phase 1 runs ECS systems on one thread, as decided in ADR 0010. A schedule runs stages in the order they were added and systems within a stage in registration order. Callers express dependencies by choosing the stage and order of registration. The executor does not infer ordering constraints or collect system access metadata.

Systems receive a shared world, a mutable command queue and a caller-owned context. The world can lend mutable component queries while entity and component membership stay stable during the stage. Each stage drains its one command queue before the next stage starts.

The ECS advances its change tick before each system and before applying stage commands. Each system receives the tick at which it last ran. A later writer in the same simulation step is then visible the next time an earlier reader runs. The application's fixed simulation step is counted separately.

## Consequences

- The same registrations and inputs give the same execution order across runs. No scheduler order depends on a hash table.
- The executor can gain access metadata when parallel execution needs it. Revisiting execution order or changing when a stage applies commands requires a new decision.
