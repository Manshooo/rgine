# ADR 0023. Development asset and scene reload

- Status: Accepted
- Date: 2026-11-26

## Decision

The opt-in app feature hot-reload runs a refresh after simulation and before render extraction. The static prop fixture fingerprints its RON scene, glTF source, external glTF buffers and optional KTX2 base color. It polls only those inputs and compares their content every 100 ms. A failed import records the files it tried to read, so creating a missing dependency retriggers import.

The app stages a fresh mesh registry when asset inputs change and reconciles the authored snapshot in the existing game World through the incremental scene compiler. It publishes the new mesh registry and scene together after validation. On a failed input the previous valid result stays active, and a later edit retries the affected input. The renderer releases GPU meshes and imported textures from the previous registry while retaining its device and pipelines.

## Verification

The [manual Linux fixture](../CONTRIBUTING.md#reload-an-authored-prop-during-development) launches the normal app loop in Xvfb with a Vulkan software adapter. It edits an external glTF buffer and a RON placement in the same running process and observes the changed pixels. The asset budget is under two seconds from the disk write to the visible change. Invalid edits exercise the same pipeline and preserve the last visible result.
