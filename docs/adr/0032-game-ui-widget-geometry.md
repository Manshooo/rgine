# ADR 0032. Game UI widget geometry

- Status: Accepted
- Date: 2026-11-27

## Decision

`engine-ui::geometry::layout` resolves a styled widget tree against a logical viewport. It uses Taffy's Flexbox algorithm for panel flow, sizing constraints and alignment. RGine maps its supported stylesheet properties to the backend and returns its own rectangles, with one box per authored widget instance. Children retain document order for drawing and hit testing.

Coordinates use a top-left origin with positive X to the right and positive Y down. Explicit sizes and constraints include padding. Margins separate sibling boxes, and the root's automatic axes fill the viewport after margins. Percentages use the corresponding axis of the parent content box when that axis has a definite size. A child with an automatic cross-axis size can stretch according to its parent's alignment.

Images take their intrinsic size from a project asset callback. Labels and buttons use `FontBook` to measure their content. The output retains the glyph runs at the final content width and an origin that includes padding and horizontal text alignment. This gives drawing the same text measurement that contributed to the box dimensions.

Layout keeps fractional logical coordinates. The renderer supplies the logical extent from its viewport and applies display scale when placing the boxes and glyphs. Hit testing uses the same half-open rectangles in logical coordinates.

## Verification

The [widget layout fixture](../game-ui-styles.md#lay-out-styled-widgets) uses two instances of a reusable card. `cargo test --locked -p engine-ui --test geometry` checks their row and column placement, sizing, alignment, spacing, scaling and measured text.

[Taffy layout engine](https://github.com/DioxusLabs/taffy)
