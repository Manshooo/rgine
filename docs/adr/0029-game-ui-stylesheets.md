# ADR 0029. Game UI stylesheets

- Status: Accepted
- Date: 2026-11-26

## Decision

`engine-ui` parses the documented CSS subset into a stylesheet and resolves it against the XML widget tree from [ADR 0028](0028-xml-widget-trees.md). Matching uses widget type, authored ID, classes, topology and caller-supplied interaction flags. The child-index path identifies the widget instance to the caller. Resolved styles carry logical size and spacing, alignment, colors and inherited text settings.

Cascade precedence uses matching selector specificity and source order. Asset replacement is transactional at the parse boundary. Invalid CSS returns a source diagnostic before a new stylesheet is available. State can change without reparsing XML or CSS.

[Game UI stylesheets](../game-ui-styles.md) records the supported syntax and defaults. The headless tests verify the cascade, repeated panels, state changes and invalid source positions.
