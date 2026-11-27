# Game UI layouts

A game supplies UTF-8 XML to `engine_ui::WidgetTree::parse(source, xml)`. The `source` argument names the asset in error messages. The result exposes the root panel, each widget's kind, ID and classes, and its children in document order.

[The HUD fixture](../crates/ui/tests/fixtures/hud.xml) declares one coin counter and places it twice. This shorter layout uses the same syntax.

```xml
<ui>
    <template name="coin-counter" class="counter">
        <image id="icon" src="ui/coin.png" />
        <label id="amount" text="Coins" />
        <button id="details" text="Details" />
    </template>

    <panel id="hud">
        <use id="player-one" template="coin-counter" class="left" />
        <use id="player-two" template="coin-counter" class="right" />
    </panel>
</ui>
```

| Element | Attributes | Children |
| --- | --- | --- |
| `ui` | | Template declarations followed by one root panel |
| `template` | Required `name`, optional `class` | Widgets |
| `panel` | Optional `id` and `class` | Widgets |
| `label` | Required `text`, optional `id`, `class` and `bind` | |
| `image` | Required `src`, optional `id` and `class` | |
| `button` | Required `text`, optional `id`, `class`, `bind` and `action` | |
| `use` | Required `template` and `id`, optional `class` | |

A `use` becomes a panel. It takes the instance's ID and appends the instance's classes to the template's classes. A template can use earlier declarations in the same file. Names and IDs are case-sensitive tokens without spaces. IDs are unique within each declaration or the layout's root scope. Both counter instances therefore have an `amount` child. To distinguish those children, use their respective instance locations in the tree.

Classes are separated by ASCII whitespace. Text and image references are attribute values, with ordinary XML escaping such as `&amp;`. Elements and attributes use unqualified names. XML syntax and layout errors include the asset name and one-based line and column. A layout supports up to 64 nested widget levels and 16,384 widgets counted across the template definitions and the expanded tree.

## Bind text and handle actions

```xml
<label text="0" bind="coins" />
<button text="Resume" action="resume" />
```

Pass a lookup for game-published strings to `engine_ui::geometry::layout_with_text`. `bind` names a value in that lookup. The authored `text` is used when the lookup has no value. Layout measures and draws the resolved string.

Enable `engine-ui`'s `input` feature to use `UiInput<A>`, where `A` is your game's action type. Call `set_layout` with the rendered widget geometry, display scale and a lookup that maps each `action` name and widget path to a typed value. Return `None` for a disabled control. Use `UiInput::state` when resolving the next styles and supply disabled flags from the game. Both attribute names take case-sensitive tokens.

Call `UiInput::route` from the `App::set_input_actions` interceptor. Dispatch any returned action and return the result's `consumed()` flag. The game can push a modal `InputContext` as it opens a menu and pop it as it closes. [ADR 0035](adr/0035-game-ui-input-and-bindings.md).

The parser runs independently of layout and rendering. The headless tests load the same fixture.

```sh
cargo test --locked -p engine-ui
```
