# ADR 0005: Compatibility and versioning policy

- Status: Accepted
- Date: 2026-09-02

## Decision
Before 0.1.0 there are no compatibility guarantees.

From 0.1.0 onwards:

- **patch** - no public API changes, no data schema changes;
- **minor** - additive changes only, to both the public engine API and data schemas;
- **major** - breaking changes allowed, data migrations mandatory.

A change is breaking if it removes or renames a public item, changes the signature or type of an existing field, changes enum variant names or discriminants that appear in serialized data, or changes the default behaviour of an existing API.

Migrations for schema version N are kept for at least two subsequent minor cycles, and always until the next major. `engine-cli` must be able to migrate a project non-interactively.

Deprecation requires the replacement to exist for one full minor cycle before removal.

### Pre-1.0 mapping
Before 1.0, cargo treats the second position as the compatibility unit, so the three levels above map onto two positions: a breaking change bumps `0.1.x` to `0.2.0`, and both additive and no-change releases bump the third position. The changelog distinguishes additive from fix-only releases even where the version number cannot. From 1.0 the mapping is one to one.

## Reasons
- Godot ships breaking changes in minor releases: add-ons built against a previous version fail at runtime with missing-method errors, and renamed enum members break compatibility. For an engine with a plugin ecosystem this costs more than the API improvement is worth.
- The Godot 3 to 4 migration is the larger warning: the automated converter produces a project that still needs substantial manual fixing, and C# API types shifted from `int` to `long` and `float` to `double`. Deciding the policy after a public release is too late.
- A stated policy is also a commercial signal. The 2023 Unity Runtime Fee changed no engine code, yet the unpredictability alone moved a measurable share of developers within days.

## Consequences
- CI needs a public API surface check (`cargo-public-api` or equivalent) and a schema snapshot test. Without them the policy is unenforceable.
- Every schema version needs a round-trip test against a stored fixture from the previous version.
- The policy applies to RON project data, the plugin trait surface and the script API (ADR 0007), not only to Rust items.
