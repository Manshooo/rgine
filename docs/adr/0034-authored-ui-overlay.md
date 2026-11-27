# ADR 0034: Authored game UI overlay

- Status: Accepted
- Date: 2026-11-27

## Decision

Prepare an owned `UiDrawList` from the logical geometry in [ADR 0032](0032-game-ui-widget-geometry.md). Paint each widget's background before its content and visit its children in XML order. Convert authored sRGB colors to linear RGB for Canvas blending and preserve their alpha. Images fill the content box using a game-provided sampled group. Text uses the same prepared placements as layout, clipped to its content box and drawn through the shared [glyph atlas](0033-game-ui-glyph-atlas.md).

The application owns fonts, images and interaction state. It can reuse the prepared list until those inputs, the authored UI or the logical viewport size change. Screen-space drawing stays in the Canvas pass over the 3D scene.

With the `hot-reload` and `ui` features, `engine-app` can follow one XML layout and one CSS stylesheet. The development watcher uses the same source fingerprints as scene reload. It reads both paths on each reload attempt and publishes the pair together after parsing succeeds. A bad edit reports the parser's source position once and leaves the previous pair active. The renderer can continue using the previous draw list across failed edits.

## Verification

The [authored overlay example](../game-ui-styles.md#render-and-reload-an-authored-overlay) shows translucent panels, an image, Latin and Cyrillic text and a menu over a 3D object. Tests cover paint order and placement at two logical viewport widths, plus successful, failed and recovered edits to the XML/CSS pair.
