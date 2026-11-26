# ADR 0026. Fixed-step 3D physics

- Status: Accepted
- Date: 2026-11-26

## Decision

`engine-physics` owns [Rapier 3D 0.36.1](https://crates.io/crates/rapier3d/0.36.1) and its handles. The Apache-2.0 dependency uses the enhanced determinism feature. The locked dependency graph includes nalgebra 0.35 and wide 1.7, which require Rust 1.89. The workspace minimum and the CI toolchain advance to Rust 1.89.

Games enable the `engine-app` feature `physics` and add a `PhysicsPlugin`. The plugin appends a fixed stage after regular gameplay updates. It uses the same delta in windowed runs, headless advances and input replay. A stage registered after the plugin can read completed poses and that step's contact events, even during multi-step catch-up.

The ECS world has its own typed resources for data associated with its lifetime. Physics installs a private inbox there. Component add and remove hooks mark affected entities, including despawn. Installation seeds the inbox from existing components. Each step reconciles the pending entities in handle order after preceding stages have applied structural commands.

An entity can have one body and one collider. With a body, the collider pose is relative to its body. With a collider alone, its configured pose is world-space and it behaves as fixed geometry. Adding or removing the body rebuilds the collider in the appropriate space. Body and collider components are construction data. Remove and reinsert a component to replace it, including within one fixed step.

The adapter publishes world-space `PhysicsPose` on entities with bodies. Rapier owns the simulated pose after construction. Gameplay reads that pose and can use it to place a renderable. A removed body loses its pose at the physics stage boundary. Contact and sensor transitions use ECS entity handles and belong to the step that reported them. A stop caused by a removal can refer to an entity that has already despawned.

## Verification

`cargo test --locked -p engine-app --features physics --test fixed_physics` checks falling, landing, sensor transitions, deferred insertion, body and collider replacement, and despawn. The same fixed-step trajectory and events are recorded under 50 Hz and 100 Hz frame schedules.
