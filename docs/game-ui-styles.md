# Game UI stylesheets

Pass UTF-8 CSS to `engine_ui::StyleSheet::parse(source, css)` and resolve it against a `WidgetTree`. `source` identifies the stylesheet in diagnostics. Resolution returns computed styles and children in the authored order, independently of rendering.

```css
.hud {
    flex-direction: column;
    gap: 8px;
    padding: 12px 20px;
    font-family: "Project UI", Noto Sans;
    font-size: 18px;
}
.hud > button:focus, .hud > button:hover {
    background-color: #4477aa;
}
```

The [HUD stylesheet fixture](../crates/ui/tests/fixtures/hud.css) styles both instances of the [XML layout](game-ui-layout.md).

## Selectors and cascade

Selectors use the widget types `panel`, `label`, `image` and `button`. They support `*`, `#id`, `.class`, descendant whitespace, direct child `>` and comma-separated lists. Compound selectors such as `button.primary:focus` match all their terms. IDs and classes are case-sensitive. Identifiers start with a letter, underscore, hyphen or non-ASCII character and may contain digits after the first character.

The selector with more ID terms wins, followed by more class and state terms and then more type terms. Source order decides ties, including repeated declarations in one rule. A selector list uses the highest specificity among its matching selectors. A style applied directly to a widget takes precedence over values inherited from its parent. [CSS cascade and specificity](https://www.w3.org/TR/CSS22/cascade.html#specificity).

A game supplies the flags for `:hover`, `:focus`, `:active` and `:disabled` to `StyleSheet::resolve`. The callback receives the child-index path from the root, which distinguishes each template instance even when local IDs repeat. Resolve the tree again when flags change. An ancestor state can affect its descendants through a selector such as `.counter:hover #amount`.

## Properties

| Properties | Accepted values |
| --- | --- |
| `width`, `height`, `min-width`, `max-width`, `min-height`, `max-height` | `auto`, nonnegative logical `px` or percentage of the parent axis |
| `margin`, `padding` | One through four nonnegative logical `px` values in CSS edge order |
| `gap` | Nonnegative logical `px` |
| `flex-direction` | `row`, `column` |
| `align-items` | `flex-start`, `center`, `flex-end`, `stretch` |
| `justify-content` | `flex-start`, `center`, `flex-end`, `space-between`, `space-around`, `space-evenly` |
| `background-color`, `color` | Hex `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, or `black`, `white`, `transparent` |
| `font-family` | Comma-separated project font families in fallback order. Names can be quoted. |
| `font-size` | Positive logical `px` |
| `font-weight` | `normal` (400), `bold` (700), or an integer from 1 through 1000 |
| `line-height` | `normal`, a nonnegative multiplier of font size, or nonnegative logical `px` |
| `text-align` | `left`, `center`, `right` |

A zero length can omit its unit. Decimal numbers are supported. Default layout uses automatic sizes and constraints, zero spacing, horizontal flow, cross-axis stretch, main-axis start alignment and a transparent background. Typography including `color` inherits. The root defaults to the project's default fonts at 16 logical pixels, weight 400, metrics-based line height, left alignment and white text. An inherited unitless line height multiplies the receiving widget's font size.

The parser accepts CSS block comments. Unsupported syntax, properties and values produce an error with the stylesheet name, line and column. A caller can retain its previous stylesheet when a replacement fails to parse.

## Measure label and button text

Register the project's font assets and pass the resolved widget typography to `FontBook::prepare`.

```rust
use engine_ui::text::FontBook;

let mut fonts = FontBook::default();
fonts.load_face("assets/fonts/ui.ttf", 0, "Project UI", 400)?;
fonts.load_face("assets/fonts/fallback.otf", 0, "Project Fallback", 400)?;
fonts.set_default_families(["Project UI", "Project Fallback"]);
let prepared = fonts.prepare("Score · Очки", &computed.typography, Some(120.0))?;
```

Use `prepared.size` for the text block's logical bounds and `prepared.lines` to place glyphs at draw time. Pass `None` for the intrinsic size without soft wrapping. Device scale applies to placement and rasterization. [ADR 0031](adr/0031-game-ui-text-layout.md).

## Lay out styled widgets

Call `engine_ui::geometry::layout` with the styled tree, the viewport's logical width and height, the project's fonts and an image size callback. Each result has outer and content rectangles. For labels and buttons, add the returned text origin to the positions of the prepared glyphs. Use `Rect::contains` with a logical pointer position and `Rect::physical` to place a rectangle in physical pixels. [ADR 0032](adr/0032-game-ui-widget-geometry.md).

## Draw prepared text

Enable the `canvas` feature of `engine-ui` and keep a `text::canvas::GlyphAtlas` for each Canvas. Call `begin_frame` in `App::with_canvas`, then pass the prepared layout, its logical origin and its color to `draw_text`. For a widget, use the origin returned with its prepared layout. The draw call uses the Canvas clip and device scale. [ADR 0033](adr/0033-game-ui-glyph-atlas.md).

Run `cargo run --locked -p engine-app --example ui_text -- 1` or pass `2` to exercise the sample at twice that scale. Omit the argument to use the window's display scale. The HUD warms all digits on frame 0 and introduces a minus sign on frame 12. The sample reports rasterizations, atlas uploads and uploaded bytes. Set `RUST_LOG=engine_render::drawable=debug` to see the Canvas draw-call count.

## Render and reload an authored overlay

Resolve the stylesheet and lay out the tree using the logical Canvas viewport. Pass the result to `engine_ui::canvas::UiDrawList::prepare` and reuse the list while the UI and logical viewport size stay the same. Draw it from `App::with_canvas` using the glyph frame for that submission. Provide image dimensions to layout and return matching sampled groups when drawing.

To try it, run `cargo run --locked -p engine-app --features hot-reload,ui --example ui_overlay`. Resize the window and edit the [sample layout](../crates/app/examples/fixtures/ui_overlay/hud.xml) or [stylesheet](../crates/app/examples/fixtures/ui_overlay/hud.css) while it runs. The example also accepts a directory containing `hud.xml` and `hud.css` after `--`, with an optional initial window width. A syntax error identifies the file, line and column. The current overlay stays visible until both files parse successfully. [ADR 0034](adr/0034-authored-ui-overlay.md).

Activate **Collect coin** to update the bound HUD value. Tab and Shift+Tab move focus. Enter and Space activate a focused button. The pause button and Escape open or close the modal menu. Move the cube with A/D or the arrow keys. Space with no focused control and a held left click on the scene also move the cube right, so you can check that activating an interface button leaves the cube in place.

The fixture puts two reusable cards in a row. Each card puts its image beside a column containing a label and button. Card width is 40% of the HUD content width with a minimum of 120 and maximum of 160 logical pixels. [XML fixture](../crates/ui/tests/fixtures/geometry.xml) · [CSS fixture](../crates/ui/tests/fixtures/geometry.css).

| Physical viewport | Scale | Logical viewport | Logical card origins | Logical card size | First card in physical pixels `(x, y, w, h)` |
| --- | ---: | --- | --- | --- | --- |
| 400 × 250 | 1.25 | 320 × 200 | (20, 60), (180, 60) | 120 × 80 | (25, 75, 150, 100) |
| 960 × 540 | 2 | 480 × 270 | (20, 95), (300, 95) | 160 × 80 | (40, 190, 320, 160) |
