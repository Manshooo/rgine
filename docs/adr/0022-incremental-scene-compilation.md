# ADR 0022. Incremental scene compilation

- Status: Accepted
- Date: 2026-11-26

## Decision

Under [ADR 0004](0004-authoring-data-model.md), `IncrementalScene` owns a development ECS world, its authored entity mappings and the last applied resolved Truth snapshot. It uses the same lowering callback and node index order as the [full compiler](0019-compiled-scene-runtime.md). The callback resolves node indices to entities when preparing components for the current update.

The compiler compares snapshots by object UUID. Adding a node, changing its properties, changing its type or moving it marks its subtree for lowering. A change to a parent's child list also marks that parent. Lookup dependencies include missing targets, so adding or deleting a target refreshes nodes that refer to it. Comparing snapshots also supports journal rewind and reloading a scene from disk.

Lowering and component preparation finish before changes are applied to existing entities. A rejected update retains the previous scene. A successful update advances the ECS change tick, keeps surviving entity handles, reconciles parent edges and updates both directions of the entity mapping. The game queues insertions and removals for its authored components. Runtime systems retain ownership of other components and leave scene entity lifetimes and parent edges to the compiler.
