# rgine Editor

```sh
cargo run --locked -p engine-editor
cargo run --locked -p engine-editor -- path/to/level.scene.ron
```

The initial scene registry supports `rgine.level_node`. Start an empty level
with **New level**. Use **Add root**, select a row, and use **Add child** to
build a hierarchy. **Rename** (F2) assigns the selected node a name. Press
Escape or click empty space in the hierarchy to clear the selection.

To reparent a node, select it and click **Move**, then click its new parent
or choose **Move to root**. Choosing a descendant reports a cycle and keeps
the pending move active, so you can choose a different parent or cancel.
**Delete** removes the selected node and its subtree after confirmation.

Use **Save** and enter a new `.scene.ron` path. Saving again writes to the
open file. **Open** accepts a scene path. Reopening the current file keeps
the selection when its object still exists. The selected UUID is shown
below the hierarchy. The editor asks before discarding unsaved edits.

The **Viewport** shows a cube for each `rgine.level_node` in the open scene.
It reframes on hierarchy edits and window resize. Older nodes with only a
name appear in a grid ordered by object ID. A node can also specify a
world-space `"position": (x, y, z)` property in the scene file. The editor
preserves that property when saving.
