# ADR 0014 — Render backend and surface lifetime

- Status: Accepted
- Date: 2026-11-23
- Tracking issue: [#37](https://github.com/Manshooo/rgine/issues/37)

## Context

Phase 2 needs to present GPU frames in windows owned by `engine-platform`. ADR 0009 keeps winit private to that crate, and `engine-render` needs the same ownership boundary for wgpu. Window, input and lifecycle events are normally delivered to the app in batches. On Android, winit requires all render surfaces to be dropped before the native `suspended` callback returns. A surface dropped during the next event batch may already be invalid.

## Decision

- Start with [wgpu 30.0.2](https://github.com/gfx-rs/wgpu/releases/tag/v30.0.2) and raise the workspace MSRV to Rust 1.87, the minimum declared by wgpu. Day-to-day development tracks stable. Enable native graphics backends for Windows, Linux, macOS and Android.
- Keep winit in `engine-platform` and wgpu in `engine-render`. The platform owns native windows and exposes an opaque, owned surface source through a renderer integration callback. The source implements the standard `raw-window-handle` traits so the renderer can use safe `Instance::create_surface`. Gameplay continues to address windows by `WindowId`. The dependency ownership rules enforce this separation.
- Deliver surface availability after window creation with its initial size in physical pixels. Deliver invalidation synchronously before the native suspend callback returns and before closing a window controlled by the platform. The renderer drops the surface and its copies of the source during invalidation. Repeated invalidation is harmless. The ordinary window events and simulation continue through the existing event batch.
- Configure only for nonzero physical dimensions, draw on redraw, and reconfigure after a resize or an outdated frame. A lost surface can be recreated from the live source. On resume the platform publishes a new source and rendering resumes on that surface.

## Consequences

Surface creation and teardown follow the window lifecycle even when simulation and redraw are batched. Native callbacks must stay responsive, so the runtime frame integration needs to account for potentially slow device initialization. The spike initializes synchronously to exercise the ownership contract.

## Surface example

`cargo run --locked -p engine-render --example surface_spike -- --smoke` exercises presentation, skipping a zero-sized drawable and recreating the surface. Run without `--smoke` to resize the window interactively. The Android CI job checks compilation of the example. Testing the lifecycle on an Android device belongs to [#38](https://github.com/Manshooo/rgine/issues/38).

The wgpu 30.0.2 example passed on Windows with a GeForce RTX 3060 and Vulkan, including native window resizing. The workspace builds on Rust 1.87 and the example compiles for Android on stable.
