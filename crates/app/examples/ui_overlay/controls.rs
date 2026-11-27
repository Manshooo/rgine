use std::{cell::RefCell, rc::Rc};

use engine_app::ecs::Entity;
use engine_app::platform::{Input, InputEvent, KeyCode, MouseButton, PhysicalKey, TouchPhase};
use engine_app::scene::Transform;
use engine_app::{ActionBinding, App, InputActions, InputContext};
use engine_ui::geometry::WidgetBox;
use engine_ui::input::{InputResult, UiInput};
use engine_ui::{Widget, WidgetState};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum UiAction {
    Collect,
    TogglePause,
}

impl UiAction {
    fn resolve(name: &str) -> Option<Self> {
        match name {
            "collect" => Some(Self::Collect),
            "toggle-pause" => Some(Self::TogglePause),
            _ => None,
        }
    }

    fn enabled(self, paused: bool) -> bool {
        self != Self::Collect || !paused
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Move {
    Left,
    Right,
}

#[derive(Default)]
pub(super) struct Controls {
    pub input: UiInput<UiAction>,
    pub coins: u32,
    pub dirty: bool,
    paused: bool,
}

impl Controls {
    pub fn text<'a>(&self, binding: &str, coins: &'a str) -> Option<&'a str> {
        match binding {
            "coins" => Some(coins),
            "mode" => Some(if self.paused { "Paused" } else { "Playing" }),
            "pause-button" => Some(if self.paused { "Resume" } else { "Pause" }),
            _ => None,
        }
    }

    pub fn state(&self, path: &[usize], widget: &Widget) -> WidgetState {
        WidgetState {
            disabled: widget
                .action()
                .and_then(UiAction::resolve)
                .is_some_and(|action| !action.enabled(self.paused)),
            ..self.input.state(path)
        }
    }

    pub fn set_layout(&mut self, root: &WidgetBox<'_, '_>, scale: f32) {
        let paused = self.paused;
        self.input.set_layout(root, scale, |_, name| {
            UiAction::resolve(name).filter(|action| action.enabled(paused))
        });
    }

    fn route(&mut self, event: &InputEvent, actions: &mut InputActions<Move>) -> bool {
        let response = self.input.route(event);
        let escape = matches!(
            event.input,
            Input::Key {
                key: PhysicalKey::Code(KeyCode::Escape),
                pressed: true,
                repeat: false
            }
        );
        let action = match response {
            InputResult::Action(action) => Some(action),
            _ if escape => Some(UiAction::TogglePause),
            _ => None,
        };
        let consumed = response.consumed() || escape;
        self.dirty |= consumed
            || matches!(
                event.input,
                Input::PointerMoved { .. }
                    | Input::MouseButton {
                        button: MouseButton::Left,
                        pressed: true
                    }
                    | Input::Touch {
                        phase: TouchPhase::Started,
                        ..
                    }
                    | Input::Cancelled
            );
        match action {
            Some(UiAction::Collect) if !self.paused => {
                self.coins = self.coins.saturating_add(1);
                eprintln!("ui overlay: collected coin {}", self.coins);
            }
            Some(UiAction::TogglePause) => {
                self.paused = !self.paused;
                if self.paused {
                    actions.push_context(InputContext::default().modal());
                } else {
                    actions.pop_context();
                }
                eprintln!("ui overlay: paused={}", self.paused);
            }
            _ => {}
        }
        consumed
    }
}

pub(super) fn install(app: &mut App, cube: Entity, controls: Rc<RefCell<Controls>>) {
    let key = |code| ActionBinding::Key(PhysicalKey::Code(code));
    let mut actions = InputActions::default();
    actions.push_context(
        InputContext::default()
            .bind(key(KeyCode::KeyA), Move::Left)
            .bind(key(KeyCode::ArrowLeft), Move::Left)
            .bind(key(KeyCode::KeyD), Move::Right)
            .bind(key(KeyCode::ArrowRight), Move::Right)
            .bind(key(KeyCode::Space), Move::Right)
            .bind(ActionBinding::MouseButton(MouseButton::Left), Move::Right),
    );
    app.set_input_actions(actions, move |event, actions, _| {
        controls.borrow_mut().route(event, actions)
    });
    app.add_system(move |world, commands, step, _| {
        let actions = step.actions::<Move>().expect("installed movement actions");
        let held = |action| f32::from(u8::from(actions.state(action).held));
        let direction = held(Move::Right) - held(Move::Left);
        if direction != 0.0 {
            let mut transform = *world.get::<Transform>(cube).expect("demo cube transform");
            transform.translation.x = (transform.translation.x
                + direction * 2.0 * step.delta().as_secs_f32())
            .clamp(-2.0, 2.0);
            commands.insert(cube, transform);
        }
    });
}
