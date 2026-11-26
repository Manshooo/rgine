# ADR 0019. Compiled scene runtime

- Status: Accepted
- Date: 2026-11-26

## Decision

Under [ADR 0004](0004-authoring-data-model.md), the full compiler assigns dense node indices in authored UUID order. The game supplies a function that lowers reflected properties to serializable spawn data. The function can resolve references to any object in the scene, including objects visited later. The artifact stores a format version, the ordered node data and parent indices. Any node indices embedded in game payloads are meaningful within that artifact.

Loading allocates a fresh ECS World and installs the hierarchy before the game applies node payloads. By that point the game can resolve node references to entities in the new world. Failed instantiation returns an error before handing the world to the caller. Game callbacks are responsible for deterministic payloads and for validating their own references.

## Build configurations

Authoring tools use the default `engine-scene` features. Debug authoring builds can instantiate a compiled scene with a bidirectional map between object UUIDs and entity handles. The map belongs to that instantiation and is kept out of the compiled artifact.

Shipped games use `engine-scene` with `default-features = false` to deserialize and instantiate `CompiledScene<T>`. The authoring types and trace are absent from this build. CI loads the same fixture that the full compiler produces using this configuration.
