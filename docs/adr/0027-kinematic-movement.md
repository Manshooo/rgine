# ADR 0027. Game-owned kinematic movement

- Status: Accepted
- Date: 2026-11-26

## Decision

The physics adapter runs Rapier's kinematic shape movement for an entity with a kinematic body, a solid collider and a `KinematicMotion` component. Gameplay requests a world-space displacement before the physics stage. The adapter uses the supplied up direction to detect support, slides along obstacles and ignores sensors and the body's own collider. It applies the adjusted displacement to the body in the fixed step and publishes the displacement and grounded state. The request is consumed once. A zero request can check for ground contact. The adapter uses Rapier's default slope limits with automatic stepping and ground snapping disabled.

When collider membership changed and the step has a movement request, the adapter refreshes Rapier's collision queries with a zero timestep before processing requests. This lets a movement see obstacles added or removed at preceding stage boundaries. All requests in that step query the same current poses.

Gameplay owns input bindings, speed, gravity, jump impulse and eligibility. It reads the grounded result from the preceding fixed step for movement decisions and `PhysicsPose` after physics for presentation.

## Verification

`cargo test --locked -p engine-app --features physics --test kinematic_replay` records a headless jump over a gap and replays the serialized input to the same sequence of poses and grounded states. A second press while airborne leaves the trajectory identical. The `fixed_physics` test suite covers obstacles added or removed immediately before a kinematic request and sensor filtering.
