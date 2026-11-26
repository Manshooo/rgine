# ADR 0025. Fixed-step input actions

- Status: Accepted
- Date: 2026-11-26

## Decision

`engine-platform` supplies physical input. `engine-app` owns the mapping from that input to a game-defined Rust action type. Its `InputActions` resolver runs independently of ECS and the window loop. Keyboard and mouse buttons produce press and release edges. Wheel events carry their original delta and units. Actions can have multiple button bindings and remain held until the last source ends.

The game pushes contexts in order of priority. For each event the resolver searches from the top, selects the first binding and stops at a modal context even if that context has no binding. A context change ends an active source if the new lookup maps it to a different action or has no match. A button already down must be released and pressed again to start the new action. Auto-repeat cannot start an action.

`App` routes each fixed step's input before running its systems. A routing callback sees every physical event in order and can consume it or change the context stack before resolving that event. A consumed press or wheel event cannot start a gameplay action. A release or window input cancellation still clears sources that were previously held. Context changes made by simulation systems take effect at the next fixed step, including on an empty batch. Ordered action events and press, hold and release state are readable throughout the step.

Focus loss and suspension produce a window-scoped input cancellation. The app retains cancellation across suspension so a resumed simulation can release the controls of the old window. The winit adapter ignores synthesized presses when focus returns. The cancellation travels in the physical batch during replay. Existing version 1 recordings remain readable. Playback uses the original router, bindings, context initialization and systems, in accordance with [ADR 0013](0013-fixed-step-input-recording.md).

## Verification

A headless fixture maps left click to selection in planet view and to construction confirmation in build mode. The wheel zooms in both. Playback repeats the ordered action stream, including the release of a held action when a modal context opens.
