# ADR 0036: Editor scene documents

- Status: Accepted
- Date: 2026-11-27

## Decision

The desktop editor keeps one authored level open. Its document owns a `Truth` and
keeps the selected `ObjectId` and file path as session state. It uses the existing
versioned scene reader and writer. Save stages a complete file beside its destination
before publishing it. New levels claim their destination without overwriting an
existing file. Open replaces the current document only after validating the new
scene and retains selection by ID when reopening the same file.

The editor uses the platform event loop and renderer Canvas for its desktop
interface. This lets the authoring panels and future scene viewport use the
same native surface. The platform supplies committed text to windows that
request it. Physical key events continue through the fixed-step input path
used by games and recordings.

The initial registry includes `rgine.level_node` with an optional authored
`name` field. Hierarchy mutations use the Truth journal API. The editor
application owns the document and view. The runtime continues to consume
compiled levels as described in [ADR 0019](0019-compiled-scene-runtime.md).
