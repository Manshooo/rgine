# Project status
**Date**: 2026-09-02
**Milestone**: 0 - Foundation, complete. Next: 1 - ECS + App Loop
**Version**: 0.1.0-dev

## Phase 0 exit
Both criteria hold:

- an empty window runs - `cargo run -p engine-app --example empty_window` opens a window and closing it returns from `run`;
- `cargo check --workspace --locked` and `cargo test --workspace --locked` pass, on top of fmt, clippy, the MSRV toolchain, `cargo deny check all` and `cargo xtask check-deps`, all required by the branch ruleset.

The cross-cutting iteration budget criterion in ROADMAP is not enforced yet; see Deferred.

## Done
- [x] Cargo workspace skeleton
- [x] Domain/app crate boundaries
- [x] Rust toolchain policy, MSRV 1.85 checked by CI
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

## Next
Phase 1 - ECS + App Loop:

- [ ] Implement type registry and reflection derive (ADR 0003) - blocks the first serialized type
- [ ] Implement real ECS storage: archetype + sparse, relations, required components, hooks
- [ ] Stages, fixed timestep, change detection, Plugin API
- [ ] Deterministic input record and replay

## Deferred
- [ ] Iteration budget harness in CI. Nothing measurable exists yet - there is no renderer, and the editor prints a line - so the harness would report the cold start of a stub and the rebuild time of an empty crate. It lands with the first phase whose budgets have content.
- [ ] `engine-cli new` creating a project from `templates/`. Not required by the phase 0 exit criterion; it belongs with the launcher and packaging work in phase 6, or earlier if the templates are needed to test the asset pipeline.
- [ ] Public API and schema snapshot checks (ADR 0005), due with phase 7.
