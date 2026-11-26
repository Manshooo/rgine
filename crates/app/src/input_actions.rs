//! Maps physical input to game-defined actions at fixed-step boundaries.

use std::collections::HashMap;

use engine_platform::{
    Input, InputEvent, MouseButton, PhysicalKey, ScrollDelta, TouchPhase, WindowId,
};

/// A physical input that can be bound in an action context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ActionBinding {
    /// A physical key position.
    Key(PhysicalKey),
    /// A mouse button.
    MouseButton(MouseButton),
    /// Mouse wheel or trackpad scroll.
    Scroll,
}

/// A set of bindings the game can push onto the active context stack.
pub struct InputContext<A> {
    bindings: HashMap<ActionBinding, A>,
    modal: bool,
}

impl<A> Default for InputContext<A> {
    fn default() -> Self {
        Self {
            bindings: HashMap::new(),
            modal: false,
        }
    }
}

impl<A> InputContext<A> {
    /// Stops unmatched input from reaching lower contexts when this context is active.
    #[must_use]
    pub fn modal(mut self) -> Self {
        self.modal = true;
        self
    }

    /// Assigns a binding, replacing any previous assignment in this context.
    #[must_use]
    pub fn bind(mut self, binding: ActionBinding, action: A) -> Self {
        self.bindings.insert(binding, action);
        self
    }
}

/// An ordered action change within a fixed step.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ActionEvent<A> {
    /// The action gained its first held button.
    Pressed(A),
    /// The action lost its last held button.
    Released(A),
    /// The wheel moved with the supplied delta and units.
    Scroll {
        /// The action bound to scroll in the winning context.
        action: A,
        /// The original scroll delta.
        delta: ScrollDelta,
    },
}

/// Button state observed after routing the current step's input.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ActionState {
    /// The action has at least one held source.
    pub held: bool,
    /// The action had a press edge during the step.
    pub pressed: bool,
    /// The action had a release edge during the step.
    pub released: bool,
}

struct Down<A> {
    window: WindowId,
    binding: ActionBinding,
    action: Option<A>,
}

/// Resolves ordered input through the active contexts without ECS or a window loop.
pub struct InputActions<A> {
    contexts: Vec<InputContext<A>>,
    down: Vec<Down<A>>,
    events: Vec<ActionEvent<A>>,
    contexts_changed: bool,
}

impl<A> Default for InputActions<A> {
    fn default() -> Self {
        Self {
            contexts: Vec::new(),
            down: Vec::new(),
            events: Vec::new(),
            contexts_changed: false,
        }
    }
}

impl<A: Copy + Eq> InputActions<A> {
    /// Adds the highest-priority context. Held sources reconcile at the next
    /// [`Self::begin_step`] or [`Self::route`], whichever comes first.
    pub fn push_context(&mut self, context: InputContext<A>) {
        self.contexts.push(context);
        self.contexts_changed = true;
    }

    /// Removes the highest-priority context, if present.
    pub fn pop_context(&mut self) -> Option<InputContext<A>> {
        let context = self.contexts.pop();
        self.contexts_changed |= context.is_some();
        context
    }

    /// Starts a fixed step, clearing the previous edges and reconciling held input.
    pub fn begin_step(&mut self) {
        self.events.clear();
        self.reconcile_contexts();
    }

    /// Routes one event in arrival order. A UI can consume presses and scroll;
    /// releases and cancellations always clean up previously held sources.
    pub fn route(&mut self, event: &InputEvent, consumed: bool) {
        self.reconcile_contexts();
        match event.input {
            Input::Key {
                key,
                pressed,
                repeat,
            } if !pressed || !repeat => {
                self.button(event.window, ActionBinding::Key(key), pressed, consumed);
            }
            Input::MouseButton { button, pressed } => {
                self.button(
                    event.window,
                    ActionBinding::MouseButton(button),
                    pressed,
                    consumed,
                );
            }
            Input::Scroll { delta, phase } if !consumed && phase != TouchPhase::Cancelled => {
                if let Some(action) = self.resolve(ActionBinding::Scroll) {
                    self.events.push(ActionEvent::Scroll { action, delta });
                }
            }
            Input::Cancelled => {
                while let Some(index) = self
                    .down
                    .iter()
                    .position(|down| down.window == event.window)
                {
                    self.end_source(index);
                    self.down.remove(index);
                }
            }
            _ => {}
        }
    }

    /// The action changes emitted so far in this step, in routing order.
    pub fn events(&self) -> &[ActionEvent<A>] {
        &self.events
    }

    /// The button state for an action. Wheel deltas are available in [`Self::events`].
    pub fn state(&self, action: A) -> ActionState {
        ActionState {
            held: self.is_held(action),
            pressed: self.events.contains(&ActionEvent::Pressed(action)),
            released: self.events.contains(&ActionEvent::Released(action)),
        }
    }

    fn resolve(&self, binding: ActionBinding) -> Option<A> {
        for context in self.contexts.iter().rev() {
            let action = context.bindings.get(&binding).copied();
            if action.is_some() || context.modal {
                return action;
            }
        }
        None
    }

    fn reconcile_contexts(&mut self) {
        if !std::mem::take(&mut self.contexts_changed) {
            return;
        }
        for index in 0..self.down.len() {
            let down = &self.down[index];
            if down.action.is_some() && down.action != self.resolve(down.binding) {
                self.end_source(index);
            }
        }
    }

    fn button(&mut self, window: WindowId, binding: ActionBinding, pressed: bool, consumed: bool) {
        let index = self
            .down
            .iter()
            .position(|down| down.window == window && down.binding == binding);
        match (pressed, index) {
            (true, None) => {
                let action = if consumed {
                    None
                } else {
                    self.resolve(binding)
                };
                if let Some(action) = action
                    && !self.is_held(action)
                {
                    self.events.push(ActionEvent::Pressed(action));
                }
                self.down.push(Down {
                    window,
                    binding,
                    action,
                });
            }
            (false, Some(index)) => {
                self.end_source(index);
                self.down.remove(index);
            }
            _ => {}
        }
    }

    fn end_source(&mut self, index: usize) {
        if let Some(action) = self.down[index].action.take()
            && !self.is_held(action)
        {
            self.events.push(ActionEvent::Released(action));
        }
    }

    fn is_held(&self, action: A) -> bool {
        self.down.iter().any(|down| down.action == Some(action))
    }
}
