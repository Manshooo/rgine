//! Optional integration of 3D physics with the app's fixed schedule.

pub use engine_physics::{
    BodyKind, Collider, ColliderShape, ContactEvent, PhysicsPose, PhysicsWorld, RigidBody,
};

use engine_core::glam::Vec3;

use crate::{App, Plugin};

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
            let delta = context.delta();
            context
                .resources_mut()
                .get_mut::<PhysicsWorld>()
                .expect("physics plugin installed its world")
                .step(world, commands, delta);
        });
    }
}
