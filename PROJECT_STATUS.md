# Project status
**Date**: 2026-11-23
**Milestone**: 1 - ECS + App Loop, complete. Next: 2 - Render v1
**Version**: 0.1.0-dev

## Phase 0 exit
Both criteria hold:

- an empty window runs - `cargo run -p engine-app --example empty_window` opens a window and closing it returns from `run`;
- `cargo check --workspace --locked` and `cargo test --workspace --locked` pass, on top of fmt, clippy, the MSRV toolchain, `cargo deny check all` and `cargo xtask check-deps`, all required by the branch ruleset.

The cross-cutting iteration budget criterion in ROADMAP is not enforced yet; see Deferred.

## Done
- [x] Cargo workspace skeleton
- [x] Domain/app crate boundaries
- [x] Rust toolchain policy and a CI job for the minimum supported Rust version
- [x] Dual MIT OR Apache-2.0 licensing
- [x] Repository name and canonical URL: `rgine`, https://github.com/Manshooo/rgine
- [x] ADR structure
- [x] CI for desktop, the Android target, MSRV, dependency policy and boundaries
- [x] Minimal core time/logging
- [x] Minimal winit bootstrap
- [x] CLI bootstrap
- [x] ADR 0003 type registry and reflection
- [x] ADR 0004 authoring data model
- [x] ADR 0005 compatibility and versioning policy
- [x] ADR 0006 no code hot reloading in 0.1
- [x] ADR 0007 gameplay scripting layer (Proposed - deferred, no scripting in 0.1)
- [x] ADR 0009 platform is a replaceable backend; `app` no longer depends on `winit`
- [x] `cargo xtask check-deps`: editor/devtools unreachable from runtime crates, backend crates named only by the crate that owns them (ADR 0009)
- [x] `cargo deny` with a permissive-only licence allow-list, fatal advisories and crates.io as the only source
- [x] Iteration budgets defined in ROADMAP
- [x] An empty window that opens and closes cleanly

## Phase 1 exit

- The fixed-update benchmark measures 1,000 and 100,000 entities in CI. On the local Windows run, the median at 1,000 entities was 1.81 µs per update.
- The deterministic replay test records and serializes input, then verifies that replay produces the same world state. The `engine-app` and `engine-ecs` test suites passed locally.
- The type registry and reflection derive, archetype and sparse storage, filtered queries, change detection, deferred commands, relationships, required components and hooks are in `master`. The app runs the deterministic ECS schedule with fixed steps, plugins and input recording.
- [Issue #23](https://github.com/Manshooo/rgine/issues/23) tracks native OS timestamps as follow-up work. The current input adapter supplies monotonic receipt timestamps with explicit provenance.

## Next

Phase 2 uses wgpu 30.0.2 with minimum Rust 1.87. [ADR 0014](docs/adr/0014-render-backend-and-surface-lifetime.md) is accepted and the [surface spike](https://github.com/Manshooo/rgine/issues/37) exercises the platform contract. [Frame integration](https://github.com/Manshooo/rgine/issues/38) is next in the [Render v1 milestone](https://github.com/Manshooo/rgine/milestone/3).

## Deferred
- [ ] Iteration budget harness in CI. The ECS benchmark reports fixed-update timing. Phase 2 will establish which renderer iteration budgets can be measured and enforced.
- [ ] `engine-cli new` creating a project from `templates/`. Not required by the phase 0 exit criterion; it belongs with the launcher and packaging work in phase 6, or earlier if the templates are needed to test the asset pipeline.
- [ ] Public API and schema snapshot checks (ADR 0005), due with phase 7.
