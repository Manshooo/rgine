# Changelog

All notable changes to rgine are recorded here, in [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

Entries are written in the pull request that makes the change, under `Unreleased`, and moved into a dated section at release time. Versioning follows [ADR 0005](docs/adr/0005-compatibility-policy.md); the pre-1.0 mapping onto version positions is in [CONTRIBUTING](docs/CONTRIBUTING.md#versioning-and-releases).

## [Unreleased]

### Added
- ADR 0003: type registry and reflection.
- ADR 0004: authoring data model, splitting `Truth` from the runtime ECS `World`.
- ADR 0005: compatibility and versioning policy.
- ADR 0006: no code hot reloading in 0.1.
- ADR 0007: gameplay scripting layer, proposed and deferred.
- ADR 0008: development workflow.
- `.gitattributes` pinning line endings and binary asset handling.
- `CHANGELOG.md`.
- Pull request and issue templates.
- `.github/setup-repo.sh`, an idempotent bootstrap for the label taxonomy and the roadmap-phase milestones.
- Iteration budgets in `ROADMAP.md`, to be enforced in CI.

### Changed
- `ARCHITECTURE.md`: two data levels, backend API rules, boundaries enforced by `xtask` rather than convention.
- `ROADMAP.md`: per-phase additions and a mod-loading track separated from scripting.
- `CONTRIBUTING.md`: full development pipeline.
- CI now triggers on `master` rather than `main`, cancels superseded pull request runs, caches the cargo registry and builds with `--locked`.
- `Cargo.lock` is committed instead of ignored, since the workspace produces binaries.
