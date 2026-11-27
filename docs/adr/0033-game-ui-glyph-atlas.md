# ADR 0033: Game UI glyph atlas

- Status: Accepted
- Date: 2026-11-27

## Decision

Expose `engine_ui::text::canvas` through the optional `canvas` feature. It takes the positioned glyphs and exact font revisions from [ADR 0031](0031-game-ui-text-layout.md) and emits textured quads through the [screen-space Canvas](0030-screen-space-canvas.md). Use the Canvas viewport's display scale to convert the font size to physical pixels. Derive logical quad offsets from each bitmap's bearings and retain the shaped positions and Canvas clipping.

Use [Skrifa](https://docs.rs/crate/skrifa/0.50.0) on the same `read-fonts` faces as the shaper to draw unhinted outlines. Fill them into coverage masks with [`ab_glyph_rasterizer`](https://docs.rs/ab_glyph_rasterizer/). Store white RGB and coverage alpha in a linear RGBA8 texture with a transparent border around each glyph for linear filtering.

Cache each bitmap by face revision, glyph index and physical font size. Allocate up to four 512 × 512 pages by default, for 4 MiB of atlas texels, and retain up to 256 empty glyph entries. Upload only the region allocated to a new bitmap. Under pressure, recycle the least recently used page among pages untouched by the current Canvas frame. Return `FrameFull` if all pages are in use and lack room, so existing commands retain their textures. `GlyphAtlas::release` frees the resources on the active device.

## Verification

The [`ui_text` sample](../game-ui-styles.md#draw-prepared-text) exercises changing digits, Cyrillic fallback and a clipped multiline label at two scales. Tests cover bitmap bearings and borders, constrained page reuse across face revisions, and native texture subregion updates.
