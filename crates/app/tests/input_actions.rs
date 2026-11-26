use std::time::Duration;

use engine_app::platform::{
    Commands, Input, InputEvent, InputTimestamp, KeyCode, MouseButton, PhysicalKey, ScrollDelta,
    TouchPhase, WindowDesc, WindowId,
};
use engine_app::{
    ActionBinding, ActionEvent, ActionState, App, InputActions, InputContext, InputRecording,
    Simulation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Select,
    Confirm,
    Zoom,
    Move,
}

const STEP: Duration = Duration::from_nanos(16_666_667);
const CLICK: ActionBinding = ActionBinding::MouseButton(MouseButton::Left);

fn windows<const N: usize>() -> [WindowId; N] {
    let mut handles = Vec::new();
    let descriptions = std::array::from_fn::<_, N, _>(|_| WindowDesc::default());
    Commands::default().create_windows(&descriptions, &mut handles);
    handles.try_into().unwrap()
}

fn event(window: WindowId, input: Input) -> InputEvent {
    InputEvent {
        window,
        timestamp: InputTimestamp::Received(Duration::ZERO),
        input,
    }
}

fn key(key: KeyCode, pressed: bool) -> Input {
    Input::Key {
        key: PhysicalKey::Code(key),
        pressed,
        repeat: false,
    }
}

fn click(pressed: bool) -> Input {
    Input::MouseButton {
        button: MouseButton::Left,
        pressed,
    }
}

#[derive(Default)]
struct History(Vec<Vec<ActionEvent<Action>>>);

fn record_actions(app: &mut App) {
    app.insert_resource(History::default());
    app.add_system(|_, _, step, _| {
        let events = step.actions::<Action>().unwrap().events().to_vec();
        step.resources_mut()
            .get_mut::<History>()
            .unwrap()
            .0
            .push(events);
    });
}

fn planet() -> Simulation {
    let mut app = App::new();
    let mut actions = InputActions::default();
    actions.push_context(
        InputContext::default()
            .bind(CLICK, Action::Select)
            .bind(
                ActionBinding::Key(PhysicalKey::Code(KeyCode::KeyB)),
                Action::Move,
            )
            .bind(ActionBinding::Scroll, Action::Zoom),
    );
    app.insert_resource(false);
    app.set_input_actions(actions, |event, actions, resources| match event.input {
        Input::Key {
            key: PhysicalKey::Code(KeyCode::KeyB),
            pressed: true,
            repeat: false,
        } => {
            actions.push_context(InputContext::default().bind(CLICK, Action::Confirm));
            true
        }
        Input::Key {
            key: PhysicalKey::Code(KeyCode::Escape),
            pressed: true,
            repeat: false,
        } => {
            let modal = resources.get_mut::<bool>().unwrap();
            if *modal {
                actions.pop_context();
            } else {
                actions.push_context(InputContext::default().modal());
            }
            *modal = !*modal;
            true
        }
        _ => false,
    });
    record_actions(&mut app);
    app.into_simulation()
}

#[test]
fn replay_reproduces_prioritized_planet_input_and_modal_releases() {
    let [window] = windows();
    let delta = ScrollDelta::Lines(0.0, 2.0);
    let wheel = Input::Scroll {
        delta,
        phase: TouchPhase::Moved,
    };
    let mut live = planet();
    live.start_recording();
    let mut step = |inputs: &[Input]| {
        live.queue_input(inputs.iter().map(|&input| event(window, input)));
        assert_eq!(live.advance(STEP), 1);
    };
    step(&[click(true), wheel]);
    step(&[key(KeyCode::KeyB, true), click(true), wheel]);
    step(&[click(false), click(true)]);
    step(&[key(KeyCode::Escape, true), wheel]);
    step(&[
        key(KeyCode::Escape, false),
        key(KeyCode::Escape, true),
        click(true),
        wheel,
    ]);
    step(&[click(false), click(true), Input::Cancelled]);

    use ActionEvent::{Pressed, Released, Scroll};
    let zoom = Scroll {
        action: Action::Zoom,
        delta,
    };
    let expected = [
        vec![Pressed(Action::Select), zoom],
        vec![Released(Action::Select), zoom],
        vec![Pressed(Action::Confirm)],
        vec![Released(Action::Confirm)],
        vec![zoom],
        vec![Pressed(Action::Confirm), Released(Action::Confirm)],
    ];
    assert_eq!(live.resources().get::<History>().unwrap().0, expected);

    let json = serde_json::to_string(&live.finish_recording().unwrap()).unwrap();
    let recording: InputRecording = serde_json::from_str(&json).unwrap();
    let mut replay = planet();
    replay.replay(&recording).unwrap();
    assert_eq!(replay.resources().get::<History>().unwrap().0, expected);
}

#[test]
fn consumed_buttons_repeat_and_multiple_sources_preserve_action_state() {
    let [first, second] = windows();
    let mut actions = InputActions::default();
    actions.push_context(InputContext::default().bind(CLICK, Action::Move).bind(
        ActionBinding::Key(PhysicalKey::Code(KeyCode::KeyW)),
        Action::Move,
    ));
    actions.begin_step();
    actions.route(&event(first, click(true)), true);
    actions.route(&event(first, click(true)), false);
    actions.route(
        &event(
            first,
            Input::Key {
                key: PhysicalKey::Code(KeyCode::KeyW),
                pressed: true,
                repeat: true,
            },
        ),
        false,
    );
    assert_eq!(actions.state(Action::Move), ActionState::default());
    actions.route(&event(first, click(false)), true);
    actions.route(&event(first, click(true)), false);
    actions.route(&event(second, key(KeyCode::KeyW, true)), false);
    assert_eq!(actions.events(), [ActionEvent::Pressed(Action::Move)]);

    actions.begin_step();
    actions.route(&event(second, Input::Cancelled), true);
    assert_eq!(
        actions.state(Action::Move),
        ActionState {
            held: true,
            ..Default::default()
        }
    );
    actions.route(&event(first, click(false)), true);
    assert_eq!(
        actions.state(Action::Move),
        ActionState {
            released: true,
            ..Default::default()
        }
    );
}

#[test]
fn system_changes_take_effect_on_the_next_step_even_without_input() {
    let [window] = windows();
    let mut app = App::new();
    let mut actions = InputActions::default();
    actions.push_context(InputContext::default().bind(CLICK, Action::Move));
    app.set_input_actions(actions, |_, _, _| false);
    app.add_system(|_, _, step, _| {
        if step.index() == 1 {
            step.actions_mut::<Action>()
                .unwrap()
                .push_context(InputContext::default().modal());
        }
    });
    record_actions(&mut app);
    let mut simulation = app.into_simulation();
    simulation.queue_input([event(window, click(true))]);
    assert_eq!(simulation.advance(3 * STEP), 3);
    assert_eq!(
        simulation.resources().get::<History>().unwrap().0,
        [
            vec![ActionEvent::Pressed(Action::Move)],
            vec![ActionEvent::Released(Action::Move)],
            vec![],
        ]
    );
}
