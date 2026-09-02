# ADR 0004: Authoring data model

- Status: Accepted
- Date: 2026-09-02

## Decision
The engine has two data levels.

Authoring data lives in `Truth`: a centralized in-memory model of objects addressed by GUID, mutated only through a change journal, serialized to RON. Runtime data lives in the ECS `World`. `Truth` is compiled into `World` deterministically and incrementally.

The editor, CLI and launcher read and write `Truth` only. They never mutate `World` directly.

## Reasons
- Authoring and runtime demand incompatible layouts. Authoring needs stable identifiers, partial data, arbitrary overrides, history and mergeability. Runtime needs linear memory without optional fields or indirection. Unity merged both into `GameObject` and had to bolt on baking as a separate compilation step, which broke the debugging loop: entities do not appear in the hierarchy and require a dedicated inspector.
- Undo, autosave, dirty tracking, prefab overrides, copy/paste and collaborative editing are implemented once against the journal instead of once per tool.
- Journal entries are `(object, property, old, new)` tuples, so undo is history rewind rather than a Command implementation per tool.
- Entering play mode becomes "compile `Truth` into an isolated `World`". Leaving play mode needs no state restoration, because `Truth` never changed. This removes the class of problem Unity pays for with domain reload.

## Consequences
- `Truth` is an authoring facility. Release builds ship compiled data and do not link it.
- Development builds keep a bidirectional `Entity` to `AuthoringId` map so a runtime entity can be traced back to the object being edited. Release builds strip the map.
- Compilation must be incremental: changing one object recompiles only the affected subtree.
- Node-local identifiers inside a scene must be stable across saves, otherwise a merge cannot correlate objects between branches and the text format provides only the illusion of mergeability.
- `Truth` depends on the type registry (ADR 0003) for schema and on GUID identity (ADR 0002) for references.
