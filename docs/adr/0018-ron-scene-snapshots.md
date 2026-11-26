# ADR 0018. RON scene snapshots

- Status: Accepted
- Date: 2026-11-26

## Decision

The initial scene format has `schema_version: 1` and an `objects` map keyed by node-local UUID. Each object stores its parent UUID, its registered `SceneTypeName`, its reflected schema version and a `properties` map keyed by reflected field name. The type name is an authored format identifier. It remains stable across Rust module moves and changes to registry order.

Properties are inline RON values. Field metadata identifies their Rust type when a consumer reads them. Object and asset GUID references keep their original values, even if a target has been deleted or is temporarily unavailable. The reader checks parent links separately because they define the scene hierarchy.

The writer orders objects by UUID and properties by name. Opening a scene rebuilds descendant links and starts a fresh Truth journal. Journal entries, registry-local type keys and runtime entities are kept in memory.

## Compatibility

The reader reports unsupported document and reflected type versions. Version 1 is the first scene format. Future version changes follow the migration policy in [ADR 0002](0002-persistent-data.md) and [ADR 0005](0005-compatibility-policy.md). The [version 1 fixture](../../crates/scene/tests/fixtures/authored-v1.scene.ron) records the original layout. [ADR 0021](0021-authored-scene-migration.md) describes the conversion to v2.
