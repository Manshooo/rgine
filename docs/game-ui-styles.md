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
