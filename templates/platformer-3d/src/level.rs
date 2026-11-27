use std::{error::Error, io};

use engine_app::core::glam::{Quat, Vec3};
use engine_app::physics::{Collider, ColliderShape, PhysicsPose};
use engine_app::scene::{Mesh, MeshRegistry, PbrMaterial, Renderable, Transform};
use engine_ecs::{Component, World};
use engine_scene::CompiledScene;

use crate::level_data::{Kind, Piece};

pub const CHECKPOINT_ACTIVE: [f32; 4] = [0.31, 0.94, 0.34, 1.0];

#[derive(Debug, Component)]
pub struct Coin {
    pub collected: bool,
    pub sensor: Collider,
    pub visual: Renderable,
}

#[derive(Debug, Component)]
pub struct Checkpoint {
    pub respawn: Vec3,
    pub inactive: Renderable,
}

pub struct Level {
    pub world: World,
    pub start: Vec3,
    pub coin_total: usize,
}

pub fn load(meshes: &mut MeshRegistry) -> Result<Level, Box<dyn Error>> {
    const COMPILED: &str = include_str!(concat!(env!("OUT_DIR"), "/level.ron"));
    let scene: CompiledScene<Piece> = ron::from_str(COMPILED)?;
    let platform = meshes.insert(Mesh::cube().with_checkerboard());
    let coin_mesh = meshes.insert(Mesh::cylinder().with_pbr_material(PbrMaterial::new(0.45, 0.34)));
    let marker = meshes.insert(Mesh::cylinder());
    let mut start = None;
    let mut coin_total = 0;
    let mut checkpoints = 0;
    let world = scene.instantiate(|world, entity, piece, _| {
        let position = Vec3::from_array(piece.position);
        let scale = Vec3::from_array(piece.size);
        let mut transform = Transform {
            translation: position,
            scale,
            ..Transform::default()
        };
        let pose = PhysicsPose {
            translation: position,
            ..PhysicsPose::default()
        };
        let (visual, collider) = match piece.kind {
            Kind::Start => {
                if start.replace(position).is_some() {
                    return Err(io::Error::other("the level has more than one start marker"));
                }
                return Ok(());
            }
            Kind::Platform => (
                Renderable {
                    mesh: platform,
                    color: [0.23, 0.39, 0.61, 1.0],
                },
                Collider::new(ColliderShape::Cuboid(scale * 0.5), pose, false),
            ),
            Kind::Coin => {
                transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                let visual = Renderable {
                    mesh: coin_mesh,
                    color: [1.0, 0.67, 0.11, 1.0],
                };
                let sensor = Collider::new(ColliderShape::Sphere(scale.x * 0.5), pose, true);
                world
                    .insert(
                        entity,
                        Coin {
                            collected: false,
                            sensor,
                            visual,
                        },
                    )
                    .unwrap();
                coin_total += 1;
                (visual, sensor)
            }
            Kind::Checkpoint => {
                let visual = Renderable {
                    mesh: marker,
                    color: [0.54, 0.31, 0.94, 1.0],
                };
                let sensor = Collider::new(ColliderShape::Cuboid(scale * 0.5), pose, true);
                world
                    .insert(
                        entity,
                        Checkpoint {
                            respawn: position + Vec3::Y * 0.04,
                            inactive: visual,
                        },
                    )
                    .unwrap();
                checkpoints += 1;
                (visual, sensor)
            }
        };
        world.insert(entity, transform).unwrap();
        world.insert(entity, visual).unwrap();
        world.insert(entity, collider).unwrap();
        Ok(())
    })?;
    let start = start.ok_or_else(|| io::Error::other("the level is missing its start marker"))?;
    if coin_total == 0 || checkpoints == 0 {
        return Err(io::Error::other("the level needs at least one coin and checkpoint").into());
    }
    Ok(Level {
        world,
        start,
        coin_total,
    })
}
