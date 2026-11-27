use std::time::Duration;

use engine_app::Simulation;
use engine_app::platform::{Commands, InputTimestamp, TouchPhase, WindowDesc, WindowId};
use engine_ui::geometry;
use engine_ui::text::TextLayout;
use engine_ui::{StyleSheet, WidgetTree};

use super::*;

const XML: &str = include_str!("../../fixtures/ui_overlay/hud.xml");
const CSS: &str = include_str!("../../fixtures/ui_overlay/hud.css");
const SCALE: f32 = 2.0;

fn fixture() -> (Simulation, Entity, WindowId, Rc<RefCell<Controls>>) {
    let mut app = App::new();
    let cube = app.world_mut().spawn_empty();
    app.world_mut().insert(cube, Transform::default()).unwrap();
    let controls = Rc::new(RefCell::new(Controls::default()));
    install(&mut app, cube, Rc::clone(&controls));
    let mut windows = Vec::new();
    Commands::default().create_windows(&[WindowDesc::default()], &mut windows);
    (app.into_simulation(), cube, windows[0], controls)
}

fn layout(controls: &mut Controls) -> ([f64; 2], [f64; 2], TextLayout) {
    let tree = WidgetTree::parse("hud.xml", XML).unwrap();
    let styles = StyleSheet::parse("hud.css", CSS).unwrap();
    let styled = styles.resolve(&tree, |path, widget| controls.state(path, widget));
    let fonts = crate::fonts().unwrap();
    let coins = controls.coins.to_string();
    let root = geometry::layout_with_text(
        &styled,
        [720.0, 480.0],
        &fonts,
        |_| None,
        |name| controls.text(name, &coins),
    )
    .unwrap();
    let centre = |node: &geometry::WidgetBox<'_, '_>| {
        let [x, y] = node.bounds.origin;
        let [w, h] = node.bounds.size;
        [(x + w * 0.5) * SCALE, (y + h * 0.5) * SCALE].map(f64::from)
    };
    let collect = centre(&root.children[1].children[1]);
    let pause = centre(&root.children[1].children[2]);
    let bound = &root.children[0].children[1].children[1].children[1];
    let text = bound.text.as_ref().unwrap();
    assert_eq!(
        text.layout,
        fonts
            .prepare(
                &coins,
                &bound.styled.style.typography,
                Some(bound.content.size[0])
            )
            .unwrap()
    );
    controls.set_layout(&root, SCALE);
    (collect, pause, text.layout.clone())
}

fn step(sim: &mut Simulation, window: WindowId, input: impl IntoIterator<Item = Input>) {
    sim.queue_input(input.into_iter().map(|input| InputEvent {
        window,
        timestamp: InputTimestamp::Received(Duration::ZERO),
        input,
    }));
    assert_eq!(sim.advance(Duration::from_nanos(16_666_667)), 1);
}

fn key(key: KeyCode, pressed: bool) -> Input {
    Input::Key {
        key: PhysicalKey::Code(key),
        pressed,
        repeat: false,
    }
}

fn pointer([x, y]: [f64; 2]) -> Input {
    Input::PointerMoved { x, y }
}

fn mouse(pressed: bool) -> Input {
    Input::MouseButton {
        button: MouseButton::Left,
        pressed,
    }
}

fn touch(id: u64, phase: TouchPhase, [x, y]: [f64; 2]) -> Input {
    Input::Touch { id, phase, x, y }
}

fn position(sim: &Simulation, cube: Entity) -> f32 {
    sim.world().get::<Transform>(cube).unwrap().translation.x
}

#[test]
fn mouse_touch_and_focused_key_activate_once_and_shape_the_published_count() {
    let (mut sim, cube, window, controls) = fixture();
    controls.borrow_mut().coins = 9;
    let (collect, pause, before) = layout(&mut controls.borrow_mut());

    step(&mut sim, window, [pointer(collect), mouse(true)]);
    assert_eq!(position(&sim, cube), 0.0);
    assert_eq!(controls.borrow().coins, 9);
    assert!(controls.borrow().input.state(&[1, 1]).active);
    step(&mut sim, window, [mouse(true), mouse(false), mouse(false)]);
    assert_eq!(controls.borrow().coins, 10);
    assert_eq!(position(&sim, cube), 0.0);
    let (_, _, after) = layout(&mut controls.borrow_mut());
    assert!(after.size[0] > before.size[0]);
    assert_ne!(
        before.lines[0].runs[0].glyphs[0].id,
        after.lines[0].runs[0].glyphs[0].id
    );

    step(
        &mut sim,
        window,
        [pointer(collect), mouse(true), pointer(pause), mouse(false)],
    );
    assert_eq!(controls.borrow().coins, 10);
    assert!(!controls.borrow().paused);
    use TouchPhase::*;
    step(
        &mut sim,
        window,
        [
            touch(1, Started, collect),
            touch(1, Started, collect),
            touch(2, Started, collect),
            touch(1, Ended, collect),
            touch(1, Ended, collect),
            touch(2, Cancelled, collect),
        ],
    );
    assert_eq!(controls.borrow().coins, 11);

    step(
        &mut sim,
        window,
        [
            Input::Cancelled,
            key(KeyCode::Tab, true),
            key(KeyCode::Tab, false),
        ],
    );
    assert!(controls.borrow().input.state(&[1, 1]).focused);
    step(&mut sim, window, [key(KeyCode::Space, true)]);
    assert_eq!(position(&sim, cube), 0.0);
    assert_eq!(controls.borrow().coins, 11);
    step(
        &mut sim,
        window,
        [
            Input::Key {
                key: PhysicalKey::Code(KeyCode::Space),
                pressed: true,
                repeat: true,
            },
            key(KeyCode::Space, false),
            key(KeyCode::Space, false),
        ],
    );
    assert_eq!(controls.borrow().coins, 12);
    assert_eq!(position(&sim, cube), 0.0);
}

#[test]
fn screen_replacement_and_window_cancellation_disarm_captured_buttons() {
    let (mut sim, cube, window, controls) = fixture();
    let (collect, _, _) = layout(&mut controls.borrow_mut());
    step(&mut sim, window, [pointer(collect), mouse(true)]);
    controls.borrow_mut().input.reset();
    let (collect, _, _) = layout(&mut controls.borrow_mut());
    step(&mut sim, window, [mouse(true), mouse(false)]);
    assert_eq!(position(&sim, cube), 0.0);
    assert_eq!(controls.borrow().coins, 0);
    step(&mut sim, window, [touch(1, TouchPhase::Started, collect)]);
    step(
        &mut sim,
        window,
        [Input::Cancelled, touch(1, TouchPhase::Ended, collect)],
    );
    assert_eq!(controls.borrow().coins, 0);
    assert_eq!(
        controls.borrow().input.state(&[1, 1]),
        WidgetState::default()
    );
}

#[test]
fn closing_the_modal_restores_movement_only_on_a_new_gameplay_press() {
    let (mut sim, cube, window, controls) = fixture();
    let (collect, _, _) = layout(&mut controls.borrow_mut());
    step(&mut sim, window, [key(KeyCode::KeyD, true)]);
    let initial = position(&sim, cube);
    assert!(initial > 0.0);
    step(
        &mut sim,
        window,
        [
            key(KeyCode::Escape, true),
            key(KeyCode::Escape, false),
            touch(7, TouchPhase::Started, collect),
        ],
    );
    assert!(controls.borrow().paused);
    let (_, resume, _) = layout(&mut controls.borrow_mut());
    step(
        &mut sim,
        window,
        [
            touch(7, TouchPhase::Ended, collect),
            key(KeyCode::KeyA, true),
        ],
    );
    assert_eq!(controls.borrow().coins, 0);
    assert_eq!(position(&sim, cube), initial);
    step(&mut sim, window, [pointer(resume), mouse(true)]);
    step(&mut sim, window, [mouse(false)]);
    assert!(!controls.borrow().paused);
    assert_eq!(position(&sim, cube), initial);
    step(&mut sim, window, []);
    assert_eq!(position(&sim, cube), initial);
    step(
        &mut sim,
        window,
        [
            key(KeyCode::KeyA, false),
            key(KeyCode::KeyD, false),
            key(KeyCode::KeyD, true),
        ],
    );
    assert!(position(&sim, cube) > initial);
}
