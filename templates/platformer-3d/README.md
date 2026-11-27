# platformer-3d

A short platformer for exercising the Phase 4 game APIs. Cross the gaps, collect the four coins and touch the violet marker to set a checkpoint. A fall before the checkpoint sends you back to the start. After activation, a fall sends you to the marker. Collected coins stay collected until you restart the level.

From the repository root, run the game.

```sh
cargo run --locked -p platformer-3d
```

| Input | Action |
| --- | --- |
| A / D or Left / Right | Move |
| Space | Jump |
| Esc or P | Pause or resume |
| Tab or Shift+Tab, then Enter or Space | Choose a pause-menu button |
| Click | Activate a pause-menu button |

Pause freezes movement and physics while the menu accepts input. **Restart level** returns to the start, clears the checkpoint and restores the coins.

Cargo resolves the platform and coin prefabs from `assets/level.scene.ron` during the build. The executable embeds the compiled scene, the XML/CSS interface and the [ABeeZee font](assets/fonts/ABeeZee-OFL.txt). The HUD, pause menu and gameplay input use the same paths in the desktop game and the fixed-step tests.

```sh
cargo test --locked -p platformer-3d
```

## Android

Use Android SDK Platform 35, Build Tools 35.0.0, the NDK, Python 3.9 or newer, and a JDK with `keytool` on the path. Set `ANDROID_HOME` to the SDK. The package script uses NDK 27.1.12297006 by default and accepts `--ndk` or `ANDROID_NDK_HOME` for a different installation. On Windows, run it from a Visual Studio Developer PowerShell.

```sh
rustup target add aarch64-linux-android
python templates/platformer-3d/android/package.py arm64-v8a
adb install -r target/platformer-android/platformer-arm64-v8a.apk
adb shell am start -n dev.rgine.platformer/android.app.NativeActivity
```

Use the `x86_64-linux-android` Rust target and `x86_64` package argument for an x86-64 emulator. The script prints the APK path and signs it with a local debug key.

Hold **Left** or **Right** and tap **Jump** with another finger. **Pause** opens the menu. Tap **Resume** or **Restart level** to continue playing. Release the previous direction and touch it again after resume.
