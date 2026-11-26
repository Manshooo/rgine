#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Fixed-step 3D physics for an ECS world.

mod backend;
mod kinematic;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use engine_core::glam::{Quat, Vec3};
use engine_ecs::{Commands, Component, Entity, World};

use backend::Backend;
pub use kinematic::{KinematicMotion, KinematicMove};

/// The version of the physics crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A rigid pose without scale. Physics writes this component after each step.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PhysicsPose {
    /// Translation in world units.
    pub translation: Vec3,
    /// Unit quaternion.
    pub rotation: Quat,
}

impl Default for PhysicsPose {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        }
    }
}

impl PhysicsPose {
    fn assert_valid(self) {
        assert!(
            self.translation.is_finite()
                && self.rotation.is_finite()
                && self.rotation.is_normalized(),
            "physics pose requires finite translation and unit rotation"
        );
    }
}

/// How a rigid body moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// Moves under gravity and through contacts.
    Dynamic,
    /// Stays at its configured position.
    Fixed,
    /// Moves by game-supplied displacement through a [`KinematicMotion`] component.
    Kinematic,
}

/// Construction data for a rigid body. Remove and reinsert to rebuild it.
#[derive(Component, Debug, Clone, Copy)]
#[component(on_add = physics_added, on_remove = body_removed)]
pub struct RigidBody {
    kind: BodyKind,
    initial_pose: PhysicsPose,
}

impl RigidBody {
    /// Creates a body at a world-space pose. A dynamic body gets its mass from
    /// its collider.
    ///
    /// # Panics
    ///
    /// If the pose has a non-finite translation or a non-unit rotation.
    #[must_use]
    pub fn new(kind: BodyKind, initial_pose: PhysicsPose) -> Self {
        initial_pose.assert_valid();
        Self { kind, initial_pose }
    }
}

/// Geometry of a collider, measured in world units.
#[derive(Debug, Clone, Copy)]
pub enum ColliderShape {
    /// Box half-extents along the local axes.
    Cuboid(Vec3),
    /// Sphere radius.
    Sphere(f32),
}

/// Construction data for one collider. Remove and reinsert to rebuild it.
#[derive(Component, Debug, Clone, Copy)]
#[component(on_add = physics_added, on_remove = collider_removed)]
pub struct Collider {
    shape: ColliderShape,
    pose: PhysicsPose,
    sensor: bool,
}

impl Collider {
    /// Creates a collider at a body-relative pose, or a world-space pose
    /// when standalone. Sensors emit overlaps without blocking motion.
    ///
    /// # Panics
    ///
    /// If the pose has a non-finite translation or a non-unit rotation, or
    /// any shape dimension is non-finite or non-positive.
    #[must_use]
    pub fn new(shape: ColliderShape, pose: PhysicsPose, sensor: bool) -> Self {
        pose.assert_valid();
        let dimensions = match shape {
            ColliderShape::Cuboid(half_extents) => half_extents,
            ColliderShape::Sphere(radius) => Vec3::splat(radius),
        };
        assert!(
            dimensions.is_finite() && dimensions.min_element() > 0.0,
            "collider dimensions must be finite and positive"
        );
        Self {
            shape,
            pose,
            sensor,
        }
    }
}

/// A transition from the latest physics step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactEvent {
    /// The two collider entities in handle order. A removed collider's entity
    /// may already be stale when a stop event is read.
    pub entities: [Entity; 2],
    /// Whether the pair started touching. False means it stopped.
    pub started: bool,
    /// Whether either collider was a sensor.
    pub sensor: bool,
}

#[derive(Default)]
struct Changes {
    body_removed: bool,
    collider_removed: bool,
}

type Pending = Arc<Mutex<BTreeMap<Entity, Changes>>>;

fn mark(world: &World, entity: Entity, update: impl FnOnce(&mut Changes)) {
    if let Some(pending) = world.resources().get::<Pending>() {
        update(pending.lock().unwrap().entry(entity).or_default());
    }
}

fn physics_added(world: &World, entity: Entity, _: &mut Commands) {
    mark(world, entity, |_| {});
}

fn body_removed(world: &World, entity: Entity, _: &mut Commands) {
    mark(world, entity, |changes| changes.body_removed = true);
}

fn collider_removed(world: &World, entity: Entity, _: &mut Commands) {
    mark(world, entity, |changes| changes.collider_removed = true);
}

/// Physics state attached to a single ECS world.
pub struct PhysicsWorld {
    backend: Backend,
    pending: Pending,
}

impl PhysicsWorld {
    /// Installs a physics lifecycle inbox and tracks components already in the world.
    ///
    /// # Panics
    ///
    /// If the world already has physics or gravity is non-finite.
    pub fn install(world: &mut World, gravity: Vec3) -> Self {
        assert!(gravity.is_finite(), "gravity must be finite");
        assert!(
            world.resources().get::<Pending>().is_none(),
            "physics is already installed"
        );
        let mut pending = BTreeMap::new();
        world.query::<&RigidBody>().for_each(|entity, _| {
            pending.entry(entity).or_default();
        });
        world.query::<&Collider>().for_each(|entity, _| {
            pending.entry(entity).or_default();
        });
        let pending = Arc::new(Mutex::new(pending));
        world.resources_mut().insert(Arc::clone(&pending));
        Self {
            backend: Backend::new(gravity),
            pending,
        }
    }

    /// Reconciles lifecycle changes and advances one fixed step. Apply the
    /// supplied commands at the stage boundary to publish newly added poses
    /// and remove poses for deleted bodies.
    ///
    /// # Panics
    ///
    /// If called with a different world or a zero duration.
    pub fn step(&mut self, world: &World, commands: &mut Commands, delta: Duration) {
        assert!(
            world
                .resources()
                .get::<Pending>()
                .is_some_and(|pending| { Arc::ptr_eq(pending, &self.pending) }),
            "physics must step its installed world"
        );
        assert!(!delta.is_zero(), "physics timestep must be positive");
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        let mut refresh_queries = !pending.is_empty();
        for (entity, changes) in pending {
            let body = world.get::<RigidBody>(entity);
            let collider = world.get::<Collider>(entity);
            if self
                .backend
                .reconcile(entity, body.as_deref(), collider.as_deref(), changes)
            {
                commands.remove::<PhysicsPose>(entity);
            }
        }
        let seconds = delta.as_secs_f32();
        world
            .query::<&mut KinematicMotion>()
            .for_each(|entity, motion| {
                motion.result = motion.requested.take().and_then(|translation| {
                    if refresh_queries {
                        self.backend.refresh_queries();
                        refresh_queries = false;
                    }
                    self.backend
                        .move_kinematic(entity, translation, motion.up, seconds)
                });
            });
        self.backend.step(seconds);
        let mut poses = world.query::<&mut PhysicsPose>();
        for (entity, pose) in self.backend.poses() {
            if poses.get(entity) != Some(&pose) {
                if let Some(current) = poses.get_mut(entity) {
                    *current = pose;
                } else {
                    commands.insert(entity, pose);
                }
            }
        }
    }

    /// The contact and sensor transitions reported by the latest fixed step.
    #[must_use]
    pub fn contacts(&self) -> &[ContactEvent] {
        self.backend.contacts()
    }
}
