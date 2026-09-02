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
- ADR 0009, recording that `platform` is a replaceable backend and is bound by the backend API rules.
- ADR 0010: the phase 1 ECS data model - entity identity, storage kind per component type, archetypes, deferred structural change, tick-based change detection, hooks and required components, and relationships with the hierarchy built on them.
- `engine-platform` public API: `PlatformApp`, `run`, `Commands`, `Command`, `Event`, `WindowEvent`, `WindowId`, `WindowDesc` and `PlatformError`, all owned by the crate and free of backend types (ADR 0009).
- `App::with_window`, so the opened window is described by the caller.
- Unit tests for command recording and for the application's loop policy; both run without an event loop.
- `xtask` crate and `cargo xtask check-deps`, asserting the boundaries in ARCHITECTURE.md over the `cargo metadata` graph: editor-side crates unreachable from runtime crates, domain crates not naming apps, and backend crates named only by the crate that owns them (ADR 0009). Wired into CI as the `boundaries` job.
- `LICENSE-MIT` alongside `LICENSE-APACHE`: the workspace is dual-licensed `MIT OR Apache-2.0`, the licensing every Rust dependency of ours already assumes.
- `deny.toml` and the `deny` CI job, running `cargo deny check all`: a permissive-only licence allow-list, fatal advisories, crates.io as the only source, and duplicate versions reported rather than fatal.
- `msrv` CI job, checking the workspace on 1.85 - the MSRV recorded in `[workspace.package]` and, until now, never tested.
- Dependency and toolchain policy in `CONTRIBUTING`, covering what raising the MSRV requires and what adding a dependency requires.
- `empty_window`, an example on `engine-app`, so the phase 0 exit criterion - an empty window opens and closes cleanly - can be run rather than asserted: `cargo run -p engine-app --example empty_window`.
- `.gitattributes` pinning line endings and binary asset handling.
- `CHANGELOG.md`.
- Pull request and issue templates.
- `.github/setup-repo.sh`, an idempotent bootstrap for the label taxonomy and the roadmap-phase milestones.
- Iteration budgets in `ROADMAP.md`, to be enforced in CI.
- `.github/rulesets/master-protect.json`, the branch ruleset as version-controlled data.

### Changed
- `engine-app` drives the event loop through `engine-platform` instead of `winit`. `App::run` now returns `Result<(), PlatformError>`; it previously returned `Result<(), winit::error::EventLoopError>`.
- `engine-platform` selects `winit`'s `android-native-activity` feature when targeting Android. The Android CI job could not build `android-activity` without it.
- `ARCHITECTURE.md`: two data levels, backend API rules extended to `platform`, boundaries enforced by `xtask` rather than convention including backend containment, and the API/schema snapshot boundary marked as not yet implemented.
- `ROADMAP.md`: per-phase additions and a mod-loading track separated from scripting.
- `CONTRIBUTING.md`: full development pipeline.
- CI now triggers on `master` rather than `main`, which is where the default branch actually is - no push to it had ever run CI. Runs superseded by a new push are cancelled, the cargo registry is cached, builds use `--locked`, and the Android job skips `xtask`, which is host-only tooling.
- `Cargo.lock` is committed instead of ignored, since the workspace produces binaries.
- `license` in `[workspace.package]` is `MIT OR Apache-2.0` rather than `Apache-2.0`.
- Engine crates depend on each other through `[workspace.dependencies]` entries carrying both a path and a version, instead of bare paths. A path dependency with no version requirement is a wildcard, which `cargo deny` rejects for a publishable crate; the version now lives in one place and a release bumps it once.
- `master-protect` requires the `boundaries`, `msrv` and `deny` checks. `cargo xtask check-deps` ran on every pull request but could not block one, so the boundaries it exists to enforce were advisory; the same would have been true of the two new checks.

### Removed
- `winit` from `crates/app/Cargo.toml`. The workspace now names the windowing backend in `crates/platform` only.
- `engine_platform::window::run_empty_window`. Opening a window is loop policy and belongs to `app`.

### Fixed
- `cargo check` for `aarch64-linux-android` failed because `android-activity` refuses to compile without a backend feature. `engine-platform` now selects `android-native-activity` for Android targets.
- A closed window left the process running. The winit backend kept the default `Wait` control flow, so the loop slept instead of iterating: the application was updated only when the OS delivered an event, and the `Exit` command recorded from that update was applied on whatever unrelated message happened to arrive next. The backend now polls, which is also what the documented "once per loop iteration" contract and a fixed timestep require.
- `actions/checkout` bumped to v5; v4 targets the deprecated Node.js 20 runtime.
