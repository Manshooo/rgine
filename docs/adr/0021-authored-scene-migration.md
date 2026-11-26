# ADR 0021. Authored scene migration

- Status: Accepted
- Date: 2026-11-26

## Decision

Authored scene and prefab schema v2 groups the registered type name and version under `object_type: (name, version)`. The pair identifies a single reflected schema and the merge driver compares it as a unit. Node UUIDs, parent UUIDs, asset GUIDs and property values keep their existing representation.

The v1 reader converts the separate `type_name` and `type_version` fields to the pair in memory. The authoring writer emits v2. The Git driver reads both formats and emits v2 for a successful merge. Type versions inside the document remain owned by the reflected registry. The compiled runtime format and the project manifest have their own versions.

`engine-cli migrate` validates `project.ron` and every `*.scene.ron` and `*.prefab.ron` in the project tree. It stages a project copy alongside the requested destination and publishes it after all files have been written. The original project is preserved. If every document is current, the command reports that state and exits successfully before writing a copy.

The [v1 fixture](../../crates/scene/tests/fixtures/authored-v1.scene.ron) and [v2 fixture](../../crates/scene/tests/fixtures/authored.scene.ron) exercise the conversion and current loader under [ADR 0002](0002-persistent-data.md) and [ADR 0005](0005-compatibility-policy.md).
