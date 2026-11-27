# ADR 0035: Game UI input and data bindings

- Status: Accepted
- Date: 2026-11-27

## Decision

Labels and buttons can name a `bind` value. Geometry resolves the game's string before shaping, so intrinsic measurement, wrapping and drawing use the same text. The XML `text` remains the fallback. Buttons can also name an `action`. The game resolves the name to its own Rust action type.

`UiInput` takes its bounds and order from the rendered widget geometry. Pointer and touch positions use the Canvas display scale. Hit testing starts with the last painted eligible button. Tab traverses eligible buttons in XML order and Shift+Tab reverses it. The focused button handles Enter and Space. A button activates once on the release of a press it captured. Releasing outside the original target, replacing the screen or cancelling the input disarms the press. Platform cancellation also clears focus and hover.

Offer physical events to the UI through the [fixed-step interceptor](0025-fixed-step-input-actions.md). The consuming result reaches `InputActions`, which still processes cleanup for held sources. The game owns menu state and pushes or pops a modal action context in the same routing callback so subsequent events observe the new context.

## Verification

The [authored overlay example](../game-ui-styles.md#render-and-reload-an-authored-overlay) binds a live coin count and uses typed actions for its pause menu. Its headless input tests run through the app's fixed-step entry point. They cover pointer, touch and keyboard activation, capture cancellation, and gameplay movement before, during and after the modal context.
