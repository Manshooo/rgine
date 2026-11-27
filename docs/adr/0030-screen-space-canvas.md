# ADR 0030. Screen-space canvas

- Status: Accepted
- Date: 2026-11-27

## Decision

`engine-render::canvas` accepts an ordered list of solid quads, textured quads and lines. Game code and UI can target the same list. `App::with_canvas` builds it from the current world before a scene redraw and supplies the drawable viewport and resource table. `Drawable::draw_scene_with_canvas` composites the list over the 3D color attachment in the same submission.

The viewport has a top-left origin with positive X to the right and positive Y down. Logical size is the physical drawable size divided by the native display scale. An affine transform maps each primitive from its local coordinates to that viewport. Clips are axis-aligned in logical viewport coordinates and are intersected on the clip stack. The physical scissor includes pixels whose centers lie inside the half-open intersection. Resizing or changing the display scale requests a redraw.

Canvas colors and texture samples use linear straight RGBA. The canvas pass loads the scene color and composites with source-over blending. It builds triangles in insertion order and batches consecutive geometry with the same sampled group and physical scissor. A new texture or clip splits the batch, and subsequent matching geometry stays in painter order. Vertices share a reusable upload buffer. Solid geometry uses a white texture through the same shader, and sampled groups use the existing resource table in either scene binding mode.

## Verification

The [canvas overlay example](../CONTRIBUTING.md#draw-a-screen-space-canvas) uses the public app API. The native offscreen test composites a transformed two-color texture, overlapping translucent quads and clipped geometry over a 3D scene. It checks pixel values at 64 × 64 with scale 1 and 128 × 96 with scale 2, and verifies that over 200 quads plus a line submit six canvas draws.
