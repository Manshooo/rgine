//! Button input and keyboard focus in logical widget geometry.

use std::collections::HashMap;

use engine_platform::{Input, InputEvent, KeyCode, MouseButton, PhysicalKey, TouchPhase};

use crate::WidgetState;
use crate::geometry::{Rect, WidgetBox};

/// The result of offering a physical event to the UI before gameplay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputResult<A> {
    /// The event can be considered by gameplay.
    Ignored,
    /// The UI handled the event without activating a button.
    Consumed,
    /// The UI handled the event and activated one button.
    Action(A),
}

impl<A> InputResult<A> {
    /// Whether gameplay should treat the event as consumed. Releases and
    /// cancellation still go through the game's action resolver for cleanup.
    pub fn consumed(&self) -> bool {
        !matches!(self, Self::Ignored)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Button<A> {
    path: Vec<usize>,
    action: A,
}

struct Target<A> {
    button: Button<A>,
    bounds: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    Mouse,
    Touch(u64),
    Key(KeyCode),
}

/// Interaction state for one screen and window. Route that window's input in
/// event order before gameplay processes it. Each button resolves to a typed
/// value supplied by the game.
pub struct UiInput<A> {
    targets: Vec<Target<A>>,
    scale: f64,
    pointer: Option<[f64; 2]>,
    hovered: Option<Button<A>>,
    focused: Option<Button<A>>,
    // An invalidated capture still owns the input sequence until release.
    captured: HashMap<Source, Option<Button<A>>>,
    shift: [bool; 2],
}

impl<A> Default for UiInput<A> {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            scale: 1.0,
            pointer: None,
            hovered: None,
            focused: None,
            captured: HashMap::new(),
            shift: [false; 2],
        }
    }
}

impl<A: Copy + Eq> UiInput<A> {
    /// Updates hit boxes and tab order from the latest rendered layout.
    /// Supply the same physical pixels per logical unit as the drawing viewport.
    /// The lookup receives the child-index path and authored action name.
    /// Return no action for disabled or unmapped buttons.
    pub fn set_layout(
        &mut self,
        root: &WidgetBox<'_, '_>,
        display_scale: f32,
        mut action: impl FnMut(&[usize], &str) -> Option<A>,
    ) {
        self.scale = f64::from(display_scale);
        self.targets.clear();
        collect(root, &mut Vec::new(), &mut action, &mut self.targets);
        self.hovered = self.pointer.and_then(|point| self.hit(point)).cloned();
        self.focused
            .take_if(|button| !contains(&self.targets, button));
        for button in self.captured.values_mut() {
            button.take_if(|button| !contains(&self.targets, button));
        }
    }

    /// Clears focus and disarms presses when replacing the screen. Captured
    /// input stays consumed until release or platform cancellation.
    pub fn reset(&mut self) {
        self.targets.clear();
        self.hovered = None;
        self.focused = None;
        for button in self.captured.values_mut() {
            *button = None;
        }
    }

    /// Returns the interaction flags for the next stylesheet resolve.
    /// The game can still set the disabled state when resolving styles.
    pub fn state(&self, path: &[usize]) -> WidgetState {
        let hovered = self.hovered.as_ref();
        WidgetState {
            hovered: hovered.is_some_and(|button| button.path == path),
            focused: self
                .focused
                .as_ref()
                .is_some_and(|button| button.path == path),
            active: self.captured.iter().any(|(source, button)| {
                button.as_ref().is_some_and(|button| {
                    button.path == path && (*source != Source::Mouse || Some(button) == hovered)
                })
            }),
            ..Default::default()
        }
    }

    /// Offers one physical event to the UI. An activation is delivered on
    /// the matching release while the original target is still eligible.
    pub fn route(&mut self, event: &InputEvent) -> InputResult<A> {
        match event.input {
            Input::PointerMoved { x, y } => {
                self.pointer = Some([x, y]);
                self.hovered = self.hit([x, y]).cloned();
                if self.captured.contains_key(&Source::Mouse) || self.hovered.is_some() {
                    InputResult::Consumed
                } else {
                    InputResult::Ignored
                }
            }
            Input::MouseButton {
                button: MouseButton::Left,
                pressed,
            } => {
                let target = self.hovered.clone();
                if pressed {
                    self.press(Source::Mouse, target, true)
                } else {
                    self.release(Source::Mouse, target.as_ref())
                }
            }
            Input::Touch { id, phase, x, y } => self.touch(id, phase, [x, y]),
            Input::Key {
                key: PhysicalKey::Code(key),
                pressed,
                repeat,
            } => self.key(key, pressed, repeat),
            Input::Cancelled => {
                self.captured.clear();
                self.focused = None;
                self.pointer = None;
                self.hovered = None;
                self.shift = [false; 2];
                InputResult::Ignored
            }
            _ => InputResult::Ignored,
        }
    }

    fn touch(&mut self, id: u64, phase: TouchPhase, point: [f64; 2]) -> InputResult<A> {
        let source = Source::Touch(id);
        match phase {
            TouchPhase::Started => {
                let target = self.hit(point).cloned();
                self.press(source, target, true)
            }
            TouchPhase::Moved if self.captured.contains_key(&source) => InputResult::Consumed,
            TouchPhase::Moved => InputResult::Ignored,
            TouchPhase::Ended => {
                let target = self.hit(point).cloned();
                self.release(source, target.as_ref())
            }
            TouchPhase::Cancelled => self.release(source, None),
        }
    }

    fn key(&mut self, key: KeyCode, pressed: bool, repeat: bool) -> InputResult<A> {
        let source = Source::Key(key);
        match key {
            KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                self.shift[usize::from(key == KeyCode::ShiftRight)] = pressed;
                InputResult::Ignored
            }
            KeyCode::Tab
                if pressed
                    && !repeat
                    && !self.targets.is_empty()
                    && !self.captured.contains_key(&source) =>
            {
                self.advance_focus();
                self.captured.insert(source, None);
                InputResult::Consumed
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space if pressed => {
                if repeat {
                    return if self.captured.contains_key(&source) {
                        InputResult::Consumed
                    } else {
                        InputResult::Ignored
                    };
                }
                self.press(source, self.focused.clone(), false)
            }
            _ if !pressed => {
                let focused = self.focused.clone();
                self.release(source, focused.as_ref())
            }
            _ if self.captured.contains_key(&source) => InputResult::Consumed,
            _ => InputResult::Ignored,
        }
    }

    fn advance_focus(&mut self) {
        let current = self.focused.as_ref().and_then(|focused| {
            self.targets
                .iter()
                .position(|target| &target.button == focused)
        });
        let count = self.targets.len();
        let next = if self.shift.into_iter().any(|held| held) {
            current.map_or(count - 1, |index| (index + count - 1) % count)
        } else {
            current.map_or(0, |index| (index + 1) % count)
        };
        self.focus(Some(self.targets[next].button.clone()));
    }

    fn press(&mut self, source: Source, target: Option<Button<A>>, focus: bool) -> InputResult<A> {
        if self.captured.contains_key(&source) {
            return InputResult::Consumed;
        }
        if focus {
            self.focus(target.clone());
        }
        if target.is_some() {
            self.captured.insert(source, target);
            InputResult::Consumed
        } else {
            InputResult::Ignored
        }
    }

    fn focus(&mut self, target: Option<Button<A>>) {
        if self.focused != target {
            self.focused = target;
            for (source, button) in &mut self.captured {
                if matches!(source, Source::Key(_)) {
                    *button = None;
                }
            }
        }
    }

    fn release(&mut self, source: Source, target: Option<&Button<A>>) -> InputResult<A> {
        match self.captured.remove(&source) {
            Some(Some(original)) if Some(&original) == target => {
                InputResult::Action(original.action)
            }
            Some(_) => InputResult::Consumed,
            None => InputResult::Ignored,
        }
    }

    fn hit(&self, point: [f64; 2]) -> Option<&Button<A>> {
        if !self.scale.is_finite() || self.scale <= 0.0 {
            return None;
        }
        let logical = point.map(|axis| (axis / self.scale) as f32);
        self.targets
            .iter()
            .rev()
            .find(|target| target.bounds.contains(logical))
            .map(|target| &target.button)
    }
}

fn contains<A: Eq>(targets: &[Target<A>], button: &Button<A>) -> bool {
    targets.iter().any(|target| &target.button == button)
}

fn collect<A>(
    node: &WidgetBox<'_, '_>,
    path: &mut Vec<usize>,
    action: &mut impl FnMut(&[usize], &str) -> Option<A>,
    targets: &mut Vec<Target<A>>,
) {
    if node.bounds.size.into_iter().all(|extent| extent > 0.0)
        && let Some(action) = node
            .styled
            .widget
            .action()
            .and_then(|name| action(path, name))
    {
        targets.push(Target {
            button: Button {
                path: path.clone(),
                action,
            },
            bounds: node.bounds,
        });
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(index);
        collect(child, path, action, targets);
        path.pop();
    }
}
