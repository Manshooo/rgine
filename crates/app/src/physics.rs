//! Optional integration of 3D physics with the app's fixed schedule.

pub use engine_physics::{
    BodyKind, Collider, ColliderShape, ContactEvent, KinematicMotion, KinematicMove,
    PhysicsInterpolation, PhysicsPose, PhysicsWorld, RigidBody,
};

use engine_core::glam::Vec3;

use crate::{App, Plugin, StepContext};

impl StepContext {
    /// Steps the installed physics world and records its CPU work during frame capture.
    pub fn step_physics(&mut self, world: &engine_ecs::World, commands: &mut engine_ecs::Commands) {
        let start = self.profile.as_ref().map(|_| std::time::Instant::now());
        let delta = self.delta();
        self.resources_mut()
            .get_mut::<PhysicsWorld>()
            .expect("physics world installed before stepping")
            .step(world, commands, delta);
        if let Some((profile, start)) = self.profile.as_mut().zip(start) {
            profile.push("physics", start);
        }
    }
}

/// Runs physics after the regular update stage, once per fixed step.
///
/// Add the plugin after registering any extra pre-physics stages and before
/// registering post-physics stages. Later stages can read the latest contacts
/// from the `PhysicsWorld` resource.
pub struct PhysicsPlugin {
    /// World-space acceleration applied to dynamic bodies.
    pub gravity: Vec3,
}

impl Default for PhysicsPlugin {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
        }
    }
}

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        let physics = PhysicsWorld::install(app.world_mut(), self.gravity);
        app.insert_resource(physics);
        let schedule = app.schedule_mut();
        let stage = schedule.add_stage();
        schedule.add_system(stage, |world, commands, context, _| {
            context.step_physics(world, commands);
        });
    }
}
