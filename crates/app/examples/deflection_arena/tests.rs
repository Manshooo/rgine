use std::time::Duration;

use engine_app::Simulation;
use engine_app::platform::{Commands as PlatformCommands, InputEvent, InputTimestamp, WindowId};

use super::*;

const STEP: Duration = Duration::from_nanos(16_666_667);

struct Harness {
    sim: Simulation,
    window: WindowId,
    paddle: Entity,
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    arena: Arena,
    paddle: Vec3,
    balls: Vec<(Entity, Vec3, bool)>,
}

impl Harness {
    fn new() -> Self {
        let (app, _, paddle) = arena_app();
        let mut windows = Vec::new();
        PlatformCommands::default().create_windows(&[WindowDesc::default()], &mut windows);
        Self {
            sim: app.into_simulation(),
            window: windows[0],
            paddle,
        }
    }

    fn step(&mut self, keys: &[(KeyCode, bool)]) {
        self.sim
            .queue_input(keys.iter().map(|&(code, pressed)| InputEvent {
                window: self.window,
                timestamp: InputTimestamp::Received(Duration::ZERO),
                input: Input::Key {
                    key: PhysicalKey::Code(code),
                    pressed,
                    repeat: false,
                },
            }));
        assert_eq!(self.sim.advance(STEP), 1);
    }

    fn advance(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step(&[]);
        }
    }

    fn snapshot(&self) -> Snapshot {
        let arena = self.sim.resources().get::<Arena>().unwrap().clone();
        let world = self.sim.world();
        let paddle = world.get::<PhysicsPose>(self.paddle).unwrap().translation;
        let mut balls = Vec::new();
        world
            .query::<(&Incoming, &PhysicsPose)>()
            .for_each(|entity, (ball, pose)| {
                balls.push((entity, pose.translation, ball.returned));
            });
        balls.sort_by_key(|&(entity, _, _)| entity);
        Snapshot {
            arena,
            paddle,
            balls,
        }
    }
}

#[test]
fn timed_strike_reverses_a_physical_ball_and_replays() {
    let mut live = Harness::new();
    assert!(live.sim.start_recording());
    live.advance(71);
    assert!(live.snapshot().arena.target_live);
    let incoming = live.snapshot().balls[0].1.x;
    live.step(&[(KeyCode::Space, true)]);
    live.step(&[(KeyCode::Space, false)]);
    live.advance(9);
    let returned = live.snapshot();
    assert_eq!(returned.arena.hits, 1, "{returned:?}");
    assert_eq!(returned.arena.misses, 0);
    assert!(returned.balls[0].2);
    assert!(returned.balls[0].1.x > incoming, "{returned:?}");

    let recording = live.sim.finish_recording().unwrap();
    let mut replay = Harness::new();
    replay.sim.replay(&recording).unwrap();
    assert_eq!(replay.snapshot(), returned);

    let mut early = Harness::new();
    early.step(&[(KeyCode::Space, true)]);
    early.step(&[(KeyCode::Space, false)]);
    early.advance(80);
    assert_eq!(early.snapshot().arena.hits, 0);
}

#[test]
fn modal_pause_reuses_keys_and_requires_a_fresh_gameplay_press() {
    use KeyCode::*;
    let mut arena = Harness::new();
    assert!(arena.sim.start_recording());
    arena.step(&[(KeyS, true)]);
    let before_pause = arena.snapshot();
    assert!(before_pause.paddle.z > 0.0);
    arena.step(&[(Escape, true), (KeyA, true)]);
    arena.advance(4);
    let paused = arena.snapshot();
    assert!(paused.arena.paused);
    assert_eq!(paused.arena.menu, MenuItem::Resume);
    assert_eq!(paused.arena.tick, before_pause.arena.tick);
    assert_eq!(paused.paddle, before_pause.paddle);
    assert_eq!(paused.balls, before_pause.balls);

    arena.step(&[(KeyS, false), (KeyS, true)]);
    assert_eq!(arena.snapshot().arena.menu, MenuItem::Restart);
    arena.step(&[(KeyW, true)]);
    assert_eq!(arena.snapshot().arena.menu, MenuItem::Resume);
    arena.step(&[(Space, true)]);
    arena.advance(2);
    let resumed = arena.snapshot();
    assert!(!resumed.arena.paused);
    assert_eq!(resumed.arena.strike_until, 0);
    assert_eq!(resumed.paddle, before_pause.paddle);
    assert_ne!(resumed.balls, before_pause.balls);
    arena.step(&[(KeyW, false), (KeyW, true)]);
    assert!(arena.snapshot().paddle.z < resumed.paddle.z);

    arena.step(&[(Escape, false), (Escape, true), (KeyS, false), (KeyS, true)]);
    assert_eq!(arena.snapshot().arena.menu, MenuItem::Restart);
    arena.step(&[(Enter, true)]);
    let restarted = arena.snapshot();
    assert_eq!(restarted.arena.tick, 0);
    assert_eq!(restarted.paddle, Vec3::Y * BODY_Y);
    assert!(restarted.balls.is_empty());

    let mut replay = Harness::new();
    replay
        .sim
        .replay(&arena.sim.finish_recording().unwrap())
        .unwrap();
    assert_eq!(replay.snapshot(), restarted);
}

#[cfg(feature = "hot-reload")]
#[test]
fn ron_edits_change_movement_curve_and_strike_transitions_in_a_running_arena() {
    use std::{fs, io, thread};

    use engine_app::hot_reload::RonReload;

    fn poll(arena: &mut Harness, reload: &mut RonReload<Tuning>) -> io::Result<bool> {
        thread::sleep(Duration::from_millis(110));
        reload.poll(
            arena
                .sim
                .world_mut()
                .resources_mut()
                .get_mut::<Tuning>()
                .unwrap(),
        )
    }

    fn expect_speed(arena: &mut Harness, keys: &[(KeyCode, bool)], speed: f32) {
        let before = arena
            .sim
            .world()
            .get::<PhysicsPose>(arena.paddle)
            .map_or(0.0, |pose| pose.translation.x);
        arena.step(keys);
        let delta = arena.snapshot().paddle.x - before;
        assert!(
            (delta - speed * STEP.as_secs_f32()).abs() < 0.0001,
            "delta {delta}"
        );
    }

    use KeyCode::{KeyD, Space};

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("arena.ron");
    fs::write(&path, tuning::BUNDLED).unwrap();
    let (mut reload, initial) = RonReload::load(path.clone(), Tuning::validate).unwrap();
    let mut arena = Harness::new();
    arena.sim.world_mut().resources_mut().insert(initial);
    expect_speed(&mut arena, &[(KeyD, true)], 6.0);

    let edited = "(
        move_speed: 12.0,
        strike_speed_curve: [(at: 0.0, speed: 18.0), (at: 0.5, speed: 30.0), (at: 1.0, speed: 6.0)],
        transitions: (strike_to_cooldown_ticks: 3, cooldown_to_ready_ticks: 2),
    )";
    fs::write(&path, edited).unwrap();
    assert!(poll(&mut arena, &mut reload).unwrap());
    expect_speed(&mut arena, &[], 12.0);
    expect_speed(&mut arena, &[(Space, true)], 18.0);
    expect_speed(&mut arena, &[(Space, false)], 30.0);
    expect_speed(&mut arena, &[], 6.0);
    expect_speed(&mut arena, &[(Space, true)], 12.0);
    assert!(!arena.snapshot().arena.striking());
    expect_speed(&mut arena, &[(Space, false)], 12.0);
    expect_speed(&mut arena, &[(Space, true)], 18.0);
    assert!(arena.snapshot().arena.striking());

    let invalid_curve = edited.replace("at: 0.5", "at: 1.0").replace("12.0", "99.0");
    for (invalid, message) in [
        ("(\n   !", "2:"),
        (&invalid_curve, "positions must increase"),
    ] {
        fs::write(&path, invalid).unwrap();
        let error = poll(&mut arena, &mut reload).unwrap_err().to_string();
        assert!(error.starts_with(&path.display().to_string()), "{error}");
        assert!(error.contains(message), "{error}");
    }
    assert!(!poll(&mut arena, &mut reload).unwrap());
    expect_speed(&mut arena, &[(Space, false)], 30.0);
    expect_speed(&mut arena, &[], 6.0);
    expect_speed(&mut arena, &[], 12.0);

    fs::write(&path, edited.replace("12.0", "10.0")).unwrap();
    assert!(poll(&mut arena, &mut reload).unwrap());
    expect_speed(&mut arena, &[], 10.0);
    assert_eq!(arena.snapshot().arena.tick, 12);
}
