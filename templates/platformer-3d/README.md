# platformer-3d

A short desktop platformer for exercising the Phase 4 game APIs. Cross the gaps, collect the four coins and touch the violet marker to set a checkpoint. A fall before the checkpoint sends you back to the start. After activation, a fall sends you to the marker. Collected coins stay collected until you restart the level.

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
