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
use kinematic::KinematicRequest;
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

/// Opts a body into retaining two fixed-step poses for presentation.
///
/// Add this component alongside a [`RigidBody`]. The first sample and the
/// first sample after body replacement use the latest pose for both endpoints.
/// Replacing a body at a respawn point therefore snaps its presentation there.
#[derive(Component, Debug, Default)]
pub struct PhysicsInterpolation(Option<[PhysicsPose; 2]>);

impl PhysicsInterpolation {
    /// Samples the two poses at a fraction from zero (previous) to one (latest).
    /// Returns `None` until physics publishes a pose or after the body is removed.
    #[must_use]
    pub fn sample(&self, alpha: f32) -> Option<PhysicsPose> {
        let [previous, current] = self.0?;
        if alpha <= 0.0 {
            return Some(previous);
        }
        if alpha >= 1.0 || previous == current {
            return Some(current);
        }
        Some(PhysicsPose {
            translation: previous.translation.lerp(current.translation, alpha),
            rotation: previous.rotation.slerp(current.rotation, alpha),
        })
    }

    /// Whether the stored samples describe motion between the two steps.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        self.0
            .is_some_and(|[previous, current]| previous != current)
    }

    /// Collapses both samples to the latest pose, for example when pausing a
    /// manually stepped physics world while the app's fixed schedule continues.
    pub fn snap_to_latest(&mut self) {
        if let Some([_, current]) = self.0 {
            self.0 = Some([current; 2]);
        }
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
    initial_velocity: Vec3,
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
        Self {
            kind,
            initial_pose,
            initial_velocity: Vec3::ZERO,
        }
    }

    /// Sets the initial world-space linear velocity of a dynamic body.
    ///
    /// # Panics
    ///
    /// If the body is not dynamic or the velocity is non-finite.
    #[must_use]
    pub fn with_linear_velocity(mut self, velocity: Vec3) -> Self {
        assert_eq!(
            self.kind,
            BodyKind::Dynamic,
            "velocity requires a dynamic body"
        );
        assert!(velocity.is_finite(), "body velocity must be finite");
        self.initial_velocity = velocity;
        self
    }
}

/// Geometry of a collider, measured in world units.
#[derive(Debug, Clone, Copy)]
pub enum ColliderShape {
    /// Box half-extents along the local axes.
    Cuboid(Vec3),
    /// Sphere radius.
    Sphere(f32),
    /// Cylinder centered on its local Y axis.
    CylinderY {
        /// Half the height along the cylinder axis.
        half_height: f32,
        /// Radius perpendicular to the axis.
        radius: f32,
    },
}

/// Construction data for one collider. Remove and reinsert to rebuild it.
#[derive(Component, Debug, Clone, Copy)]
#[component(on_add = physics_added, on_remove = collider_removed)]
pub struct Collider {
    shape: ColliderShape,
    pose: PhysicsPose,
    sensor: bool,
    restitution: f32,
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
            ColliderShape::CylinderY {
                half_height,
                radius,
            } => Vec3::new(radius, half_height, radius),
        };
        assert!(
            dimensions.is_finite() && dimensions.min_element() > 0.0,
            "collider dimensions must be finite and positive"
        );
        Self {
            shape,
            pose,
            sensor,
            restitution: 0.0,
        }
    }

    /// Sets how much relative speed is preserved when this collider bounces.
    ///
    /// # Panics
    ///
    /// If the coefficient is outside the inclusive range zero to one.
    #[must_use]
    pub fn with_restitution(mut self, coefficient: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&coefficient),
            "restitution must be in [0, 1]"
        );
        self.restitution = coefficient;
        self
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
    /// and remove poses for deleted bodies. Updates opted-in interpolation
    /// samples from completed physics poses.
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
        let mut interpolation = world.query::<&mut PhysicsInterpolation>();
        for (entity, changes) in pending {
            let body = world.get::<RigidBody>(entity);
            let collider = world.get::<Collider>(entity);
            if changes.body_removed
                && let Some(samples) = interpolation.get_mut(entity)
            {
                samples.0 = None;
            }
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
                motion.result = motion.requested.take().and_then(|request| {
                    if refresh_queries && matches!(request, KinematicRequest::Sweep(_)) {
                        self.backend.refresh_queries();
                        refresh_queries = false;
                    }
                    self.backend
                        .move_kinematic(entity, request, motion.up, seconds)
                });
            });
        self.backend.step(seconds);
        let mut poses = world.query::<&mut PhysicsPose>();
        for (entity, pose) in self.backend.poses() {
            if let Some(samples) = interpolation.get(entity) {
                let previous = samples.0.map_or(pose, |[_, current]| current);
                let updated = Some([previous, pose]);
                if samples.0 != updated {
                    interpolation.get_mut(entity).unwrap().0 = updated;
                }
            }
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
