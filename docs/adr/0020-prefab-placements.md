# ADR 0020. Linked prefab placements

- Status: Accepted
- Date: 2026-11-26

## Decision

A prefab source is a versioned RON Truth document with one root. The same type registry loads the source and the containing scene. A scene stores each placement as an authored object with its own UUID, its parent, a source asset GUID, the expected source root UUID and local overrides keyed by source node and reflected field. The ordinary scene format and journal record placements and their edits.

Resolution takes a source lookup by asset GUID and returns a separate Truth snapshot for compilation. The placement UUID becomes the resolved root UUID. A descendant UUID is UUIDv5 using the placement UUID as namespace and the source node UUID's bytes as its name. This keeps instance IDs stable across saves, repeated resolution and source property edits. Authors can address a descendant before resolution through `PrefabInstance::node_id`.

Inherited `ObjectId` and `Option<ObjectId>` fields refer to nodes in the source and are translated to scene IDs. Local overrides already address scene IDs. Other field types, including asset GUIDs, keep their stored values. Authoring v1 supports one source level. Unavailable assets, invalid roots, unresolved source references, stale override targets and generated ID collisions report the offending placement.

The runtime scene format stays unchanged. The scene compiler consumes resolved nodes using the existing scene references and hierarchy.
