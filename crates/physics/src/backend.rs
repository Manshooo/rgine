use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use engine_core::glam::{Quat, Vec3};
use engine_ecs::Entity;
use rapier3d::control::KinematicCharacterController;
use rapier3d::prelude as rapier;

use crate::kinematic::KinematicRequest;
use crate::{
    BodyKind, Changes, Collider, ColliderShape, ContactEvent, KinematicMove, PhysicsPose, RigidBody,
};

#[derive(Default)]
struct Handles {
    body: Option<rapier::RigidBodyHandle>,
    collider: Option<rapier::ColliderHandle>,
}

pub(super) struct Backend {
    world: rapier::PhysicsWorld,
    handles: BTreeMap<Entity, Handles>,
    collider_entities: HashMap<rapier::ColliderHandle, Entity>,
    removed_colliders: Vec<rapier::ColliderHandle>,
    event_collector: rapier::ChannelEventCollector,
    collision_events: Mutex<Receiver<rapier::CollisionEvent>>,
    contacts: Vec<ContactEvent>,
}

impl Backend {
    pub(super) fn new(gravity: Vec3) -> Self {
        let (sender, receiver) = channel();
        Self {
            world: rapier::PhysicsWorld {
                gravity: rapier::Vector::from_array(gravity.to_array()),
                ..rapier::PhysicsWorld::default()
            },
            handles: BTreeMap::new(),
            collider_entities: HashMap::new(),
            removed_colliders: Vec::new(),
            event_collector: rapier::ChannelEventCollector::new(sender, channel().0, channel().0),
            collision_events: Mutex::new(receiver),
            contacts: Vec::new(),
        }
    }

    pub(super) fn reconcile(
        &mut self,
        entity: Entity,
        body: Option<&RigidBody>,
        collider: Option<&Collider>,
        changes: Changes,
    ) -> bool {
        let handles = self.handles.entry(entity).or_default();
        let body_changed = changes.body_removed || (handles.body.is_none() && body.is_some());
        if (body_changed || changes.collider_removed)
            && let Some(handle) = handles.collider.take()
        {
            self.world.remove_collider(handle);
            self.removed_colliders.push(handle);
        }
        if changes.body_removed
            && let Some(handle) = handles.body.take()
        {
            self.world.remove_body(handle);
        }
        if let (None, Some(body)) = (handles.body, body) {
            let builder = match body.kind {
                BodyKind::Dynamic => rapier::RigidBodyBuilder::dynamic(),
                BodyKind::Fixed => rapier::RigidBodyBuilder::fixed(),
                BodyKind::Kinematic => rapier::RigidBodyBuilder::kinematic_position_based(),
            };
            handles.body = Some(
                self.world.insert_body(
                    builder
                        .pose(to_backend_pose(body.initial_pose))
                        .linvel(rapier::Vector::from_array(body.initial_velocity.to_array())),
                ),
            );
        }
        if let (None, Some(collider)) = (handles.collider, collider) {
            let builder = match collider.shape {
                ColliderShape::Cuboid(size) => {
                    rapier::ColliderBuilder::cuboid(size.x, size.y, size.z)
                }
                ColliderShape::Sphere(radius) => rapier::ColliderBuilder::ball(radius),
                ColliderShape::CylinderY {
                    half_height,
                    radius,
                } => rapier::ColliderBuilder::cylinder(half_height, radius),
            };
            let builder = builder
                .position(to_backend_pose(collider.pose))
                .restitution(collider.restitution)
                .sensor(collider.sensor)
                .active_events(rapier::ActiveEvents::COLLISION_EVENTS);
            let handle = self.world.insert_collider(builder, handles.body);
            handles.collider = Some(handle);
            self.collider_entities.insert(handle, entity);
        }
        if handles.body.is_none() && handles.collider.is_none() {
            self.handles.remove(&entity);
        }
        changes.body_removed && body.is_none()
    }

    pub(super) fn refresh_queries(&mut self) {
        // Reconciled colliders must be visible before the requested movement.
        // A zero timestep updates Rapier's queries and contacts without integration.
        self.world.integration_parameters.dt = 0.0;
        self.world.step_with_events(&(), &self.event_collector);
    }

    pub(super) fn move_kinematic(
        &mut self,
        entity: Entity,
        request: KinematicRequest,
        up: Vec3,
        seconds: f32,
    ) -> Option<KinematicMove> {
        let handles = self.handles.get(&entity)?;
        let body_handle = handles.body?;
        let body = &self.world.bodies[body_handle];
        let collider = &self.world.colliders[handles.collider?];
        if !body.is_kinematic() || collider.is_sensor() {
            return None;
        }

        let (translation, grounded) = match request {
            KinematicRequest::Direct(translation) => {
                (rapier::Vector::from_array(translation.to_array()), false)
            }
            KinematicRequest::Sweep(translation) => {
                self.sweep(body_handle, collider, translation, up, seconds)
            }
        };
        let next = body.translation() + translation;
        self.world.bodies[body_handle].set_next_kinematic_translation(next);
        Some(KinematicMove {
            translation: Vec3::from_array(translation.to_array()),
            grounded,
        })
    }

    fn sweep(
        &self,
        body_handle: rapier::RigidBodyHandle,
        collider: &rapier::Collider,
        translation: Vec3,
        up: Vec3,
        seconds: f32,
    ) -> (rapier::Vector, bool) {
        let query = self.world.query_pipeline_with_filter(
            rapier::QueryFilter::default()
                .exclude_rigid_body(body_handle)
                .exclude_sensors(),
        );
        let controller = KinematicCharacterController {
            up: rapier::Vector::from_array(up.to_array()),
            snap_to_ground: None,
            ..KinematicCharacterController::default()
        };
        let movement = controller.move_shape(
            seconds,
            &query,
            collider.shape(),
            collider.position(),
            rapier::Vector::from_array(translation.to_array()),
            |_| {},
        );
        (movement.translation, movement.grounded)
    }

    pub(super) fn step(&mut self, seconds: f32) {
        self.world.integration_parameters.dt = seconds;
        self.world.step_with_events(&(), &self.event_collector);
        self.contacts.clear();
        for event in self.collision_events.get_mut().unwrap().try_iter() {
            let mut entities = [
                self.collider_entities[&event.collider1()],
                self.collider_entities[&event.collider2()],
            ];
            entities.sort_unstable();
            self.contacts.push(ContactEvent {
                entities,
                started: event.started(),
                sensor: event.sensor(),
            });
        }
        // Rapier can emit a stop during the step for a collider removed above.
        for handle in self.removed_colliders.drain(..) {
            self.collider_entities.remove(&handle);
        }
    }

    pub(super) fn poses(&self) -> impl Iterator<Item = (Entity, PhysicsPose)> + '_ {
        self.handles.iter().filter_map(|(&entity, handles)| {
            let body = &self.world.bodies[handles.body?];
            let pose = PhysicsPose {
                translation: Vec3::from_array(body.translation().to_array()),
                rotation: Quat::from_array(body.rotation().to_array()),
            };
            Some((entity, pose))
        })
    }

    pub(super) fn contacts(&self) -> &[ContactEvent] {
        &self.contacts
    }
}

fn to_backend_pose(pose: PhysicsPose) -> rapier::Pose {
    rapier::Pose::from_parts(
        rapier::Vector::from_array(pose.translation.to_array()),
        rapier::Rotation::from_array(pose.rotation.to_array()),
    )
}
