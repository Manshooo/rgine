# Phase 4 platformer verification

## Desktop and Android route

| Target | Development build | Result |
|---|---|---|
| Windows 11, GeForce RTX 3060 | DX12, classic bindings, `cargo run --locked -p platformer-3d --features dev-reload -- --dev $dev` | Two coins collected, checkpoint activated, pause and resume worked, and the player respawned at `x=4.85` with `2/4` coins. |
| Android 35, Pixel 7 AVD, x86-64 | Emulator 37.3.10, host graphics, Vulkan, `python templates/platformer-3d/android/package.py x86_64` | The same route passed with simultaneous direction and jump touches. Pause and Resume worked by touch, and the player respawned at `x=4.85` with `2/4` coins. |

From the repository root, follow the [platformer README](../templates/platformer-3d/README.md) to prepare the desktop asset directory or package the Android library. For the Android run, install and start the APK on the selected emulator.

```sh
adb -s emulator-5580 install -r target/platformer-android/platformer-x86_64.apk
adb -s emulator-5580 shell am start -W -n dev.rgine.platformer/android.app.NativeActivity
```

1. Start from a fresh level. Hold Right and collect the first coin. Jump just before the edge of the first gap while continuing to hold Right.
2. Land on the second platform, collect its coin, and continue to the violet checkpoint. Release Right once the checkpoint turns green.
3. Pause with Esc on Windows or the Pause button on Android. Resume with Tab and Enter or tap Resume.
4. Walk off the second platform into the next gap. Release Right during the fall. Confirm that the checkpoint stays active and the counter still reads `2/4` after respawn.

The debug build logs coin counts, the direction and speed at takeoff, pause state, the checkpoint and the respawn position. The Android verification sent two touchscreen pointers through UiAutomation. A separate run cancelled a held Right touch, then tapped Jump and observed direction `0` at takeoff. The Vulkan setup skips debug object names on x86-64 Android to avoid the emulator driver's crash when naming swapchain texture views. An arm64 APK also built and passed signature verification. Device execution used the x86-64 AVD. [Android UiAutomation](https://developer.android.com/reference/android/app/UiAutomation)

## Live edits in the Windows run

The same process continued after each edit to the copied files.

| File | Edit | Observed result |
|---|---|---|
| `tuning.ron` | Set `jump_speed` to `8.0` from `6.4`. | The next jump logged speed `8` at the same checkpoint. |
| `hud.xml` | Change the counter label from `COINS` to `GEMS`. | The label changed in the next capture while the count remained `2/4`. |
| `ui.css` | Change the HUD background from `#102439ee` to `#1e6538ee`. | The composited panel pixel changed from RGB `(19, 38, 59)` to `(31, 99, 58)`. |

## Regression checks

`cargo test --locked -p platformer-3d --all-features` exercises the normal fixed-step path, including the keyboard route, pause during a jump, independent touch IDs, release and cancellation, and modal input isolation. The Android arm64 CI job builds the NativeActivity library. Workspace tests including the shader variant budget, Clippy, dependency boundaries, and the Rust 1.89 build check passed on Windows.

The existing DX12 reload checks measured 130.4 ms for an external glTF buffer, 207.7 ms for a scene edit, and 243.6 ms for a shader edit. The local runner raised the test window before sampling its pixels. The 2 s asset and 1 s shader budgets held. The existing incremental build test measured a median of 3.607 s against the 10 s budget. [Reload and rebuild commands](phase-3-exit.md)
