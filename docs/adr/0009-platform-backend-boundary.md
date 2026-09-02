# ADR 0009: Platform is a replaceable backend

- Status: Accepted
- Date: 2026-09-02

## Context
`platform` was described in ARCHITECTURE.md as the OS/window/input abstraction, but the backend API rules were written to cover `render`, `physics`, `audio` and `script` only. `platform` was left out, and the leak followed immediately: `app` depended on `winit` directly and named `winit::error::EventLoopError` in the return type of `App::run`. Every caller of that function - `engine-cli`, `editor`, `launcher` and any game built on the engine - was therefore coupled to the windowing backend, which is the exact failure the rules exist to prevent.

Windowing is not a settled choice. Android is a first-class target from Phase 0, the editor will need multiple windows, and a headless backend is required to run the loop in CI. Each of those is a backend question, and none of them can be answered while `winit` types are part of the engine's public API.

## Decision
The backend API rules apply to `platform` on the same terms as the other backends. Concretely:

- `platform` owns its error type (`PlatformError`), its window handle (`WindowId`), its window description (`WindowDesc`) and its event vocabulary (`Event`, `WindowEvent`). None of them wraps or exposes a backend type.
- Windows are addressed by opaque handle. The backend owns the window objects; a caller never holds one.
- Requests to the platform are recorded into `Commands` and applied by the backend after the application returns. Nothing on that type returns a backend value, so no per-frame call is a synchronization point. Handles for windows being created are allocated by the queue itself, which is why creation can stay one-way.
- The application-facing entry point is batched: `PlatformApp::update` receives every event accumulated since the previous call as a slice, rather than one callback per event.
- `app` owns loop policy - which windows exist, when to exit, how to react to suspend - and reaches the OS only through the above.

## Consequences
- `app` no longer depends on `winit`. The dependency exists in exactly one crate in the workspace.
- Replacing or adding a backend (a headless one for CI, a direct Android activity backend) is a change inside `platform` with no caller edits.
- The Android activity backend selection that `winit` requires is a `platform` manifest detail rather than something every downstream crate has to know about.
- A regression of this kind is invisible to `cargo check`. The planned `xtask` dependency-graph check is extended to assert that no crate outside the owning one names a backend crate as a dependency.
