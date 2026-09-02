# ADR 0003: Type registry and reflection

- Status: Accepted
- Date: 2026-09-02

## Decision
A single derive-based type registry in `core` is the only source of type metadata. Every type that is authored, serialized, inspected, migrated, replicated or exposed to scripts registers its fields, field types, attributes and schema version through one `derive` macro.

## Reasons
- The inspector, RON serialization, schema migrations, undo deltas, script bindings and the generated reference all need the same metadata. Implemented separately they drift apart.
- Unreal demonstrates the payoff: `UPROPERTY` drives detail panels, serialization, garbage collection, network replication and Blueprint interop from one mechanism. Without it each of those layers is written again.
- Rust proc-macros run inside the compiler, so this avoids the UnrealHeaderTool failure mode, where an external tool parses only a subset of the language and silently constrains the type system.
- The registry must exist before the first serialized type. Added later, it forces a rewrite of everything already persisted.

## Consequences
- `core` gains a proc-macro dependency. The registry is populated during initialization and must be available in headless builds.
- Human-facing metadata (display names, tooltips, ranges) is feature-gated so release builds do not carry editor strings.
- Schema version lives in the registry, so the migrations required by ADR 0002 are driven by registered data rather than hand-written per format.
- Script type definitions are generated from the registry, not maintained by hand (ADR 0007).
- Adding a field to a registered type is a schema change and is governed by ADR 0005.
