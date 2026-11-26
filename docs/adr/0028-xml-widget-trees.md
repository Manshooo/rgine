# ADR 0028. XML widget trees

- Status: Accepted
- Date: 2026-11-26

## Decision

`engine-ui` turns a UTF-8 layout asset into an immutable tree of panels, labels, images and buttons. The tree preserves painter order, authored identifiers and class lists. The game can hold its own runtime state by widget location. Each template instance starts a scope for its descendants' local IDs. Earlier template declarations can be reused in later declarations, which makes expansion acyclic by construction.

Parsing takes the asset's text and name and returns a complete tree or a source diagnostic. The caller owns reading the asset and can keep its current tree when an edit fails. Styling, input and drawing can consume the same tree independently. The parser reuses the MIT-licensed `xml-rs` package already present in the workspace lockfile. It supplies XML tokenization, entity decoding and source locations.

[Game UI layouts](../game-ui-layout.md) documents the grammar. The headless fixture verifies instance scopes, child order, Unicode text and malformed source locations.
