use std::time::Duration;

use engine_app::Simulation;
use engine_app::canvas::Viewport;
use engine_app::core::glam::Vec3;
use engine_app::ecs::Entity;
use engine_app::physics::{Collider, KinematicMotion, PhysicsInterpolation, PhysicsPose};
use engine_app::platform::{
    Commands, Input, InputEvent, InputTimestamp, KeyCode, PhysicalKey, WindowDesc, WindowId,
};
use engine_app::scene::Renderable;

use crate::game::{Game, Run};
use crate::level::{CHECKPOINT_ACTIVE, Checkpoint, Coin};
use crate::ui::Overlay;

const STEP: Duration = Duration::from_nanos(16_666_667);

struct Fixture {
    simulation: Simulation,
    player: Entity,
    window: WindowId,
    overlay: Overlay,
}

impl Fixture {
    fn new() -> Self {
        let Game {
            app,
            player,
            interaction,
            ..
        } = Game::build().unwrap();
        let mut windows = Vec::new();
        Commands::default().create_windows(&[WindowDesc::default()], &mut windows);
        let mut fixture = Self {
            simulation: app.into_simulation(),
            player,
            window: windows[0],
            overlay: Overlay::new(interaction).unwrap(),
        };
        for _ in 0..12 {
            fixture.step([]);
        }
        assert!(fixture.grounded());
        fixture
    }

    fn step(&mut self, keys: impl IntoIterator<Item = (KeyCode, bool)>) {
        self.simulation
            .queue_input(keys.into_iter().map(|(key, pressed)| InputEvent {
                window: self.window,
                timestamp: InputTimestamp::Received(Duration::ZERO),
                input: Input::Key {
                    key: PhysicalKey::Code(key),
                    pressed,
                    repeat: false,
                },
            }));
        assert_eq!(self.simulation.advance(STEP), 1);
        let run = self.simulation.world().get::<Run>(self.player).unwrap();
        self.overlay
            .prepare(&run, Viewport::new([1040, 640], 1.0).unwrap())
            .unwrap();
    }

    fn tap(&mut self, key: KeyCode) {
        self.step([(key, true), (key, false)]);
    }

    fn position(&self) -> Vec3 {
        self.simulation
            .world()
            .get::<PhysicsPose>(self.player)
            .unwrap()
            .translation
    }

    fn grounded(&self) -> bool {
        self.simulation
            .world()
            .get::<KinematicMotion>(self.player)
            .and_then(|motion| motion.result())
            .is_some_and(|motion| motion.grounded)
    }

    fn wait_for(&mut self, message: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..160 {
            if done(self) {
                return;
            }
            self.step([]);
        }
        panic!(
            "{message}; stopped at {:?}, collected {} coins",
            self.position(),
            self.coins()
        );
    }

    fn walk_to(&mut self, x: f32) {
        let right = x > self.position().x;
        let key = if right { KeyCode::KeyD } else { KeyCode::KeyA };
        self.step([(key, true)]);
        self.wait_for(
            "expected to reach the requested horizontal position",
            |test| {
                if right {
                    test.position().x >= x
                } else {
                    test.position().x <= x
                }
            },
        );
        self.step([(key, false)]);
    }

    fn jump_to(&mut self, x: f32) {
        assert!(self.grounded(), "takeoff at {:?}", self.position());
        self.step([(KeyCode::KeyD, true), (KeyCode::Space, true)]);
        self.step([(KeyCode::Space, false)]);
        self.wait_for("expected to land after crossing a gap", |test| {
            test.position().x >= x && test.grounded()
        });
        self.step([(KeyCode::KeyD, false)]);
    }

    fn coins(&self) -> usize {
        self.simulation
            .world()
            .get::<Run>(self.player)
            .unwrap()
            .coins
    }

    fn checkpoint(&self) -> Option<Entity> {
        self.simulation
            .world()
            .get::<Run>(self.player)
            .unwrap()
            .checkpoint
    }

    fn paused(&self) -> bool {
        self.simulation
            .world()
            .get::<Run>(self.player)
            .unwrap()
            .paused
    }
}

#[test]
fn keyboard_route_collects_every_coin_and_retains_progress_on_checkpoint_respawn() {
    let mut game = Fixture::new();
    let start = game.position();
    let entity_count = game.simulation.world().entity_count();
    game.walk_to(-2.45);
    assert_eq!(game.coins(), 1);
    game.walk_to(-3.95);
    assert_eq!(
        game.coins(),
        1,
        "walking through an old coin never collects it again"
    );

    game.walk_to(-0.05);
    game.jump_to(3.3);
    game.walk_to(5.0);
    assert_eq!(game.coins(), 2);
    let checkpoint = game.checkpoint().expect("crossed the checkpoint");
    let marker = game
        .simulation
        .world()
        .get::<Checkpoint>(checkpoint)
        .unwrap();
    let expected_spawn = marker.respawn;
    let inactive = marker.inactive;
    drop(marker);
    assert_eq!(
        game.simulation
            .world()
            .get::<Renderable>(checkpoint)
            .unwrap()
            .color,
        CHECKPOINT_ACTIVE
    );

    game.step([(KeyCode::KeyD, true)]);
    game.wait_for(
        "walking into the second gap should eventually fall",
        |test| {
            test.simulation
                .world()
                .get::<Run>(test.player)
                .unwrap()
                .respawn_pending
        },
    );
    assert!(game.position().y < -4.0);
    game.step([(KeyCode::KeyD, false)]);
    assert_eq!(game.position(), expected_spawn);
    let samples = game
        .simulation
        .world()
        .get::<PhysicsInterpolation>(game.player)
        .unwrap();
    assert_eq!(samples.sample(0.0), samples.sample(1.0));
    drop(samples);
    assert_eq!(game.coins(), 2);
    assert_eq!(game.checkpoint(), Some(checkpoint));

    game.wait_for("respawn should ground the player", Fixture::grounded);
    game.walk_to(6.4);
    game.jump_to(9.6);
    game.walk_to(12.45);
    assert_eq!(game.coins(), 3);
    game.jump_to(15.9);
    game.walk_to(17.0);
    assert_eq!(game.coins(), 4);
    assert_eq!(game.simulation.world().entity_count(), entity_count);
    game.simulation
        .world()
        .query::<&Coin>()
        .for_each(|entity, coin| {
            assert!(coin.collected);
            assert!(game.simulation.world().get::<Renderable>(entity).is_none());
            assert!(game.simulation.world().get::<Collider>(entity).is_none());
        });

    game.tap(KeyCode::Escape);
    assert!(game.paused());
    game.tap(KeyCode::Tab);
    game.tap(KeyCode::Tab);
    game.tap(KeyCode::Enter);
    assert!(!game.paused());
    assert!(game.position().x == start.x && game.position().y > start.y);
    assert_eq!(game.coins(), 0);
    assert_eq!(game.checkpoint(), None);
    assert_eq!(
        *game
            .simulation
            .world()
            .get::<Renderable>(checkpoint)
            .unwrap(),
        inactive
    );
    assert_eq!(game.simulation.world().entity_count(), entity_count);
    game.simulation
        .world()
        .query::<&Coin>()
        .for_each(|entity, coin| {
            assert!(!coin.collected);
            assert!(game.simulation.world().get::<Renderable>(entity).is_some());
            assert!(game.simulation.world().get::<Collider>(entity).is_some());
        });
    game.walk_to(-2.45);
    assert_eq!(game.coins(), 1, "restarted coins have working sensors");
}

#[test]
fn pausing_mid_jump_freezes_player_and_resume_uses_the_same_world() {
    let mut game = Fixture::new();
    game.walk_to(-2.35);
    game.tap(KeyCode::Space);
    let frozen = game.position();
    assert!(!game.grounded());
    game.tap(KeyCode::Escape);
    assert!(game.paused());
    game.step([(KeyCode::KeyD, true)]);
    for _ in 0..45 {
        game.step([]);
    }
    assert_eq!(game.position(), frozen);
    assert_eq!(game.coins(), 1);
    game.tap(KeyCode::Tab);
    game.tap(KeyCode::Space);
    assert!(!game.paused());
    assert_eq!(
        game.position().x,
        frozen.x,
        "the held key cannot start movement after the modal"
    );
    assert!(game.position().y > frozen.y, "the paused jump continues");
    assert_eq!(game.coins(), 1);
    game.step([(KeyCode::KeyD, false), (KeyCode::KeyD, true)]);
    assert!(game.position().x > frozen.x);
}
