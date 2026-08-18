# ADR 0002: Persistent project data

- Status: Accepted
- Date: 2026-08-18

## Decision
Project/scene/prefab metadata uses text-oriented RON with explicit schema versions and migrations. Asset identity uses GUIDs rather than filesystem paths.

## Consequences
Files remain reviewable and merge-friendly, while migrations can evolve project data without requiring engine-side compatibility hacks.
