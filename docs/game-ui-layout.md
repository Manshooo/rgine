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
| `label` | Required `text`, optional `id` and `class` | |
| `image` | Required `src`, optional `id` and `class` | |
| `button` | Required `text`, optional `id` and `class` | |
| `use` | Required `template` and `id`, optional `class` | |

A `use` becomes a panel. It takes the instance's ID and appends the instance's classes to the template's classes. A template can use earlier declarations in the same file. Names and IDs are case-sensitive tokens without spaces. IDs are unique within each declaration or the layout's root scope. Both counter instances therefore have an `amount` child. To distinguish those children, use their respective instance locations in the tree.

Classes are separated by ASCII whitespace. Text and image references are attribute values, with ordinary XML escaping such as `&amp;`. Elements and attributes use unqualified names. XML syntax and layout errors include the asset name and one-based line and column. A layout supports up to 64 nested widget levels and 16,384 widgets counted across the template definitions and the expanded tree.

The parser runs independently of layout and rendering. The headless tests load the same fixture.

```sh
cargo test --locked -p engine-ui
```
