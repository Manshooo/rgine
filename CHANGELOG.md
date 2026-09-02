# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to the compatibility policy in ADR 0005.

## [Unreleased]

### Added
- `xtask` crate and `cargo xtask check-deps`, asserting the boundaries in ARCHITECTURE.md over the `cargo metadata` graph: editor-side crates unreachable from runtime crates, domain crates not naming apps, and backend crates named only by the crate that owns them (ADR 0008). Wired into CI as the `boundaries` job.
- `engine-platform` public API: `PlatformApp`, `run`, `Commands`, `Command`, `Event`, `WindowEvent`, `WindowId`, `WindowDesc` and `PlatformError`, all owned by the crate and free of backend types (ADR 0008).
- ADR 0008, recording that `platform` is a replaceable backend and is bound by the backend API rules.
- `App::with_window`, so the opened window is described by the caller.
- Unit tests for command recording and for the application's loop policy; both run without an event loop.

### Changed
- `engine-app` drives the event loop through `engine-platform` instead of `winit`. `App::run` now returns `Result<(), PlatformError>`; it previously returned `Result<(), winit::error::EventLoopError>`.
- `engine-platform` selects `winit`'s `android-native-activity` feature when targeting Android. The Android CI job could not build `android-activity` without it.
- CI triggers on pushes to `master`; it watched `main`, which is not the default branch here; no push to the default branch had ever run CI. The Android job skips `xtask`, which is host-only tooling.
- ARCHITECTURE.md extends the backend API rules to `platform`, adds the backend-containment rule to the enforced boundaries, and marks the API/schema snapshot boundary as not yet implemented.

### Removed
- `winit` from `crates/app/Cargo.toml`. The workspace now names the windowing backend in `crates/platform` only.
- `engine_platform::window::run_empty_window`. Opening a window is loop policy and belongs to `app`.
