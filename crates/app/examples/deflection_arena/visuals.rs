use engine_app::core::glam::{Mat4, Vec3};
use engine_app::ecs::{Commands, Entity, World};
use engine_app::scene::{Camera, Mesh, MeshKey, MeshRegistry, PbrMaterial, Renderable, Transform};

use super::{Arena, MenuItem, TARGET_RADIUS};

const INK: [f32; 4] = [0.79, 0.88, 0.92, 1.0];
const MUTED: [f32; 4] = [0.35, 0.46, 0.56, 1.0];
const TEAL: [f32; 4] = [0.13, 0.79, 0.68, 1.0];
const AMBER: [f32; 4] = [0.96, 0.54, 0.16, 1.0];
const ZONE_IDLE: [f32; 4] = [0.065, 0.39, 0.42, 1.0];

struct Part {
    transform: Transform,
    renderable: Renderable,
}

impl Part {
    fn spawn(self, world: &mut World) -> Entity {
        let entity = world.spawn_empty();
        world.insert(entity, self.transform).unwrap();
        world.insert(entity, self.renderable).unwrap();
        entity
    }
}

fn shape(mesh: MeshKey, translation: Vec3, scale: Vec3, color: [f32; 4]) -> Part {
    Part {
        transform: Transform {
            translation,
            scale,
            ..Transform::default()
        },
        renderable: Renderable { mesh, color },
    }
}

fn rect(mesh: MeshKey, x: f32, z: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Part {
    shape(
        mesh,
        Vec3::new(x, y, z),
        Vec3::new(width, 1.0, height),
        color,
    )
}

pub(super) fn scene(world: &mut World) -> (MeshRegistry, MeshKey, MeshKey, Overlay) {
    let mut meshes = MeshRegistry::default();
    let plane = meshes.insert(Mesh::plane());
    let disc = meshes.insert(Mesh::cylinder());
    let paddle = meshes.insert(Mesh::cylinder().with_pbr_material(PbrMaterial::new(0.28, 0.44)));
    let ball = meshes.insert(Mesh::sphere());
    let camera = world.spawn_empty();
    let (_, rotation, translation) = Mat4::look_at_rh(Vec3::Y * 10.2, Vec3::ZERO, Vec3::NEG_Z)
        .inverse()
        .to_scale_rotation_translation();
    world.insert(camera, Camera::default()).unwrap();
    world
        .insert(
            camera,
            Transform {
                translation,
                rotation,
                ..Transform::default()
            },
        )
        .unwrap();

    let zone = shape(
        disc,
        Vec3::Y * 0.03,
        Vec3::new(TARGET_RADIUS * 2.0, 0.022, TARGET_RADIUS * 2.0),
        ZONE_IDLE,
    )
    .spawn(world);
    for part in [
        rect(
            plane,
            0.0,
            0.0,
            0.0,
            16.8,
            11.15,
            [0.025, 0.046, 0.073, 1.0],
        ),
        rect(
            plane,
            0.0,
            0.0,
            0.012,
            16.0,
            9.5,
            [0.036, 0.069, 0.104, 1.0],
        ),
        rect(
            plane,
            0.0,
            -4.76,
            0.027,
            16.0,
            0.042,
            [0.14, 0.23, 0.33, 1.0],
        ),
        rect(
            plane,
            0.0,
            4.76,
            0.027,
            16.0,
            0.042,
            [0.14, 0.23, 0.33, 1.0],
        ),
        shape(
            disc,
            Vec3::Y * 0.046,
            Vec3::new(
                TARGET_RADIUS * 2.0 - 0.085,
                0.014,
                TARGET_RADIUS * 2.0 - 0.085,
            ),
            [0.033, 0.112, 0.137, 1.0],
        ),
        rect(plane, 0.0, 0.0, 0.06, 0.42, 0.022, [0.18, 0.32, 0.37, 1.0]),
        rect(plane, 0.0, 0.0, 0.06, 0.022, 0.42, [0.18, 0.32, 0.37, 1.0]),
    ] {
        part.spawn(world);
    }
    let grid = [0.065, 0.11, 0.15, 1.0];
    for x in -7..=7 {
        rect(plane, x as f32, 0.0, 0.022, 0.016, 9.45, grid).spawn(world);
    }
    for z in -4..=4 {
        rect(plane, 0.0, z as f32, 0.024, 15.95, 0.016, grid).spawn(world);
    }
    for part in text(
        plane,
        "DEFLECTION ARENA",
        Vec3::new(-7.94, 0.065, -5.34),
        0.102,
        INK,
    )
    .chain(text(
        plane,
        "WASD MOVE / SPACE STRIKE / ESC PAUSE",
        Vec3::new(-7.94, 0.065, 5.01),
        0.087,
        MUTED,
    )) {
        part.spawn(world);
    }

    (
        meshes,
        paddle,
        ball,
        Overlay {
            plane,
            zone,
            target_live: false,
            counters: None,
            selection: None,
            strike_status: None,
        },
    )
}

impl Marker {
    fn replace(self, commands: &mut Commands, world: &World, parts: impl Iterator<Item = Part>) {
        world.query::<&OverlayMarker>().for_each(|entity, group| {
            if group.0 == self {
                commands.despawn(entity);
            }
        });
        for part in parts {
            part.enqueue_marked(commands, self);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    Scores,
    Menu,
    Status,
}

#[derive(engine_app::ecs::Component)]
struct OverlayMarker(Marker);

impl Part {
    fn enqueue_marked(self, commands: &mut Commands, marker: Marker) {
        commands
            .spawn_empty()
            .insert(self.transform)
            .insert(self.renderable)
            .insert(OverlayMarker(marker));
    }
}

pub(super) struct Overlay {
    plane: MeshKey,
    zone: Entity,
    target_live: bool,
    counters: Option<[u32; 2]>,
    selection: Option<MenuItem>,
    strike_status: Option<&'static str>,
}

impl Overlay {
    pub(super) fn update(&mut self, world: &World, commands: &mut Commands, arena: &Arena) {
        let plane = self.plane;
        if self.target_live != arena.target_live {
            self.target_live = arena.target_live;
            world
                .query::<&mut Renderable>()
                .get_mut(self.zone)
                .unwrap()
                .color = if arena.target_live { TEAL } else { ZONE_IDLE };
        }
        let counters = [arena.hits, arena.misses];
        if self.counters != Some(counters) {
            self.counters = Some(counters);
            let hits = format!("HITS {:02}", arena.hits.min(99));
            let misses = format!("MISS {:02}", arena.misses.min(99));
            Marker::Scores.replace(
                commands,
                world,
                text(plane, &hits, Vec3::new(1.9, 0.065, -5.34), 0.096, TEAL).chain(text(
                    plane,
                    &misses,
                    Vec3::new(5.25, 0.065, -5.34),
                    0.096,
                    AMBER,
                )),
            );
        }

        let (label, color) = if arena.striking() {
            ("STRIKE", AMBER)
        } else if arena.tick < arena.recovery_until {
            ("COOLDOWN", MUTED)
        } else {
            ("READY", TEAL)
        };
        if self.strike_status != Some(label) {
            self.strike_status = Some(label);
            Marker::Status.replace(
                commands,
                world,
                text(plane, label, Vec3::new(5.08, 0.065, 5.01), 0.087, color),
            );
        }

        let selection = arena.paused.then_some(arena.menu);
        if self.selection != selection {
            self.selection = selection;
            Marker::Menu.replace(
                commands,
                world,
                selection.into_iter().flat_map(|item| {
                    let active = |entry| if item == entry { TEAL } else { MUTED };
                    [
                        rect(plane, 0.0, 0.0, 0.76, 6.58, 3.95, [0.17, 0.37, 0.44, 1.0]),
                        rect(
                            plane,
                            0.0,
                            0.0,
                            0.77,
                            6.48,
                            3.85,
                            [0.023, 0.052, 0.083, 1.0],
                        ),
                    ]
                    .into_iter()
                    .chain(text(
                        plane,
                        "PAUSED",
                        Vec3::new(-1.49, 0.79, -1.55),
                        0.13,
                        INK,
                    ))
                    .chain(text(
                        plane,
                        "RESUME",
                        Vec3::new(-1.23, 0.79, -0.53),
                        0.107,
                        active(MenuItem::Resume),
                    ))
                    .chain(text(
                        plane,
                        "RESTART",
                        Vec3::new(-1.42, 0.79, 0.20),
                        0.107,
                        active(MenuItem::Restart),
                    ))
                    .chain(text(
                        plane,
                        "W S SELECT / SPACE CONFIRM",
                        Vec3::new(-2.59, 0.79, 1.19),
                        0.052,
                        MUTED,
                    ))
                }),
            );
        }
    }
}

fn text(
    mesh: MeshKey,
    value: &str,
    origin: Vec3,
    pixel: f32,
    color: [f32; 4],
) -> impl Iterator<Item = Part> + '_ {
    value
        .bytes()
        .enumerate()
        .flat_map(move |(letter, character)| {
            glyph(character)
                .into_iter()
                .enumerate()
                .flat_map(move |(row, bits)| {
                    (0..3)
                        .filter(move |&column| bits & (4 >> column) != 0)
                        .map(move |column| {
                            rect(
                                mesh,
                                origin.x + (letter * 4 + column) as f32 * pixel,
                                origin.z + row as f32 * pixel,
                                origin.y,
                                pixel * 0.86,
                                pixel * 0.86,
                                color,
                            )
                        })
                })
        })
}

fn glyph(character: u8) -> [u8; 5] {
    match character {
        b'A' => [2, 5, 7, 5, 5],
        b'C' => [3, 4, 4, 4, 3],
        b'D' => [6, 5, 5, 5, 6],
        b'E' => [7, 4, 6, 4, 7],
        b'F' => [7, 4, 6, 4, 4],
        b'H' => [5, 5, 7, 5, 5],
        b'I' => [7, 2, 2, 2, 7],
        b'K' => [5, 5, 6, 5, 5],
        b'L' => [4, 4, 4, 4, 7],
        b'M' => [5, 7, 7, 5, 5],
        b'N' => [5, 7, 7, 7, 5],
        b'O' => [7, 5, 5, 5, 7],
        b'P' => [6, 5, 6, 4, 4],
        b'R' => [6, 5, 6, 5, 5],
        b'S' => [3, 4, 2, 1, 6],
        b'T' => [7, 2, 2, 2, 2],
        b'U' => [5, 5, 5, 5, 7],
        b'V' => [5, 5, 5, 5, 2],
        b'W' => [5, 5, 7, 7, 5],
        b'Y' => [5, 5, 2, 2, 2],
        b'0' => [7, 5, 5, 5, 7],
        b'1' => [2, 6, 2, 2, 7],
        b'2' => [6, 1, 7, 4, 7],
        b'3' => [6, 1, 3, 1, 6],
        b'4' => [5, 5, 7, 1, 1],
        b'5' => [7, 4, 6, 1, 6],
        b'6' => [3, 4, 7, 5, 7],
        b'7' => [7, 1, 2, 2, 2],
        b'8' => [7, 5, 7, 5, 7],
        b'9' => [7, 5, 7, 1, 6],
        b'/' => [1, 1, 2, 4, 4],
        _ => [0; 5],
    }
}
