# Project status
**Date**: 2026-09-02
**Milestone**: 0 - Foundation
**Version**: 0.1.0-dev

## Done
- [x] Cargo workspace skeleton
- [x] Domain/app crate boundaries
- [x] Rust toolchain policy
- [x] MIT/Apache-2.0 placeholders
- [x] ADR structure
- [x] CI skeleton for desktop + Android target
- [x] Minimal core time/logging
- [x] Minimal winit bootstrap
- [x] CLI bootstrap
- [x] ADR 0003 type registry and reflection
- [x] ADR 0004 authoring data model
- [x] ADR 0005 compatibility and versioning policy
- [x] ADR 0006 no code hot reloading in 0.1
- [x] ADR 0007 gameplay scripting layer (Proposed - deferred, no scripting in 0.1)
- [x] Iteration budgets defined in ROADMAP

# Next
- [ ] Decide project/repository name and canonical Git URL
- [ ] Confirm Rust MSRV policy
- [ ] Implement type registry and reflection derive (ADR 0003) - blocks the first serialized type
- [ ] Implement real ECS storage: archetype + sparse, relations, required components, hooks
- [ ] Add xtask dependency-graph check: editor/devtools unreachable from runtime crates
- [ ] Add iteration budget harness to CI
- [ ] Add cargo-deny
- [ ] Add dependency/license policy
- [ ] Make engine-cli new create a project
- [ ] Complete desktop/Android CI setup
