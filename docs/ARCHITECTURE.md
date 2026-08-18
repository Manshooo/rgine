# Architecture
Dependency direction:

`apps -> app -> domain crates`

Domain crates must not depend on `apps`.

Core layers:

**1.** `core` - foundational types, time, jobs, logging.

**2.** `ecs` - data-oriented world and scheduler.

**3.** `platform` - OS/window/input abstraction.

**4.** `render`, `physics`, `audio` - replaceable backends.

**5.** `asset`, `scene`, `script` - data/runtime services.

**6.** `app` - integration, plugins, lifecycle.

**7.** `engine-cli`, `editor`, `launcher` - clients of the public engine API.


Editor-only functionality must never become a runtime dependency.
