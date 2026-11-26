# ADR 0024. Development shader reload

- Status: Accepted
- Date: 2026-11-26

## Decision

With the app's `hot-reload` feature and a shader directory configured, the game checks its scene WGSL inputs every 100 ms. The renderer recompiles prepared pipelines that use the changed sources and publishes the replacements together after validation. The render device and resource tables stay active. An invalid edit reports the changed source files and the compiler diagnostic, and the previous pipelines keep drawing while the developer corrects the files.

## Verification

The [manual Linux fixture](../CONTRIBUTING.md#reload-an-authored-prop-during-development) uses a Vulkan software adapter and Xvfb. It checks that invalid WGSL leaves the red prop visible, then replaces the broken source with a shader that renders the prop green. It records the machine and the time from the disk edit to the first green frame. The check fails at one second or more.
