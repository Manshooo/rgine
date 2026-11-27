# ADR 0031. Game UI text layout

- Status: Accepted
- Date: 2026-11-27

## Decision

`engine-ui::text::FontBook` loads project-owned TrueType and OpenType faces from files or bytes. A project registers each face by family name, weight and collection index. A computed CSS family list selects the nearest registered weight in each family, with the lower weight resolving ties. The project supplies the default family list for styles that omit it.

Use HarfRust with its `read-fonts` backend for OpenType shaping and metrics. This keeps font handling in Rust and works headlessly on the workspace targets. Resolve fallback per extended grapheme cluster in family order, then itemize the text by face, Unicode script and direction. A missing cluster becomes one replacement character, with the first face's `.notdef` glyph as the final fallback. Glyph clusters retain offsets into the original UTF-8 string.

`FontBook::prepare` returns logical line advances, baselines, heights and positioned runs. The layout width is the greatest line advance. CSS text alignment positions lines within that width. Normal line height uses the largest face extents and gap in each line. The renderer can scale the prepared glyph positions and font size for the display. Each run retains the bytes and identity of the exact face revision, including after a replacement.

Wrapping uses Unicode line-break opportunities and falls back to extended grapheme boundaries for long words. A single cluster can exceed the requested width. Final lines are reshaped at their boundaries. Hard breaks and empty lines keep their line boxes. Automatic wraps trim trailing spaces and tabs at a break.

## Verification

`cargo test --locked -p engine-ui` checks mixed Latin and Cyrillic fallback, weight selection, kerning, combining marks, invalid assets, missing glyphs and a styled label at two widths and scales. The font fixtures include independently licensed TrueType and CFF OpenType faces.

[HarfRust](https://crates.io/crates/harfrust/0.16.2) · [Unicode line breaking](https://github.com/axelf4/unicode-linebreak) · [Unicode bidirectional text](https://github.com/servo/unicode-bidi)
