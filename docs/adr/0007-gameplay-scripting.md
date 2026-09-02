# ADR 0007: Gameplay scripting layer

- Status: Proposed - deferred, not adopted for 0.1
- Date: 2026-09-02
- Review: at the end of Phase 3

## Context
Rust is the right language for engine and simulation code, but it imposes rebuild latency on gameplay iteration, and ADR 0006 rules out code hot reloading as the answer. The question is whether the engine needs a separate authoring language for gameplay, and if so which one.

## Decision
rgine 0.1 ships no scripting layer and no general-purpose visual scripting.

Gameplay is written in Rust. Iteration pressure is absorbed by the ladder in "What we do instead" below.

If a trigger condition fires, the default candidate is Luau embedded through `mlua`, in an optional `script` backend crate, with bindings and `.d.luau` type definitions generated from the type registry (ADR 0003). Adopting it converts this ADR to Accepted; until then no script API surface is designed or exposed.

## Trigger conditions
Any one of these opens the decision. None of them is anticipated during Phases 0-3.

1. **Iteration budget breached.** The gameplay crate cannot hold the < 10 s incremental rebuild budget on a project of realistic size, after the mitigations are exhausted: gameplay isolated in its own small crate, engine crates dynamically linked in dev builds, tuning moved to data.
2. **A non-Rust author.** Someone who does not write Rust needs to author gameplay behaviour.
3. **Third-party logic.** A game shipped on rgine needs to accept logic mods, and an embedded VM becomes the sandbox boundary.

Absent a trigger, the correct action is to do nothing. A scripting layer built speculatively binds an API that is still moving and has to be redesigned.

## What we do instead
In order. Each rung is cheaper than the next and absorbs most of the pressure that would otherwise reach it.

1. **Type registry and reflection** (ADR 0003). Everything above depends on it, including any future graph or script layer. This is the only rung scheduled for Phase 1.
2. **Data with hot reload.** Configs, curves, tuning values, ability definitions in RON. Most of what feels like a need for scripting is a need to change numbers and graphs without rebuilding.
3. **Domain-specific graphs, on demand.** A state machine, a behaviour tree, a material graph - each narrow, each justified on its own, each added only when a concrete need appears.
4. **A text scripting language.** Only on a trigger above.

## Rejected alternatives

### General-purpose visual scripting (a Blueprint analogue)
Rejected. The track record outside Unreal is uniformly bad, and Unreal itself is moving away from it.

- **Godot removed VisualScript in 4.0.** Their stated reason: despite continuous effort it never gained traction, the path to improve it never became clear, and the approach taken from the start was simply not the right one. Keeping it in core was also judged to be engine bloat. The code was moved to a separate repository. Visual *shaders* were explicitly kept - see the distinction below.
- **Unity Visual Scripting is in maintenance mode.** Bolt was acquired, Bolt 2 was cancelled, and the package sits with limited development. The performance gap is the telling number: C# is roughly 33x faster than the visual scripting runtime.
- **Epic is repositioning Blueprint as a frontend to a text language.** Verse was designed for scale and user-generated-content safety, which neither Blueprint nor C++ provided; the stated direction around UE6 is a visual interface *for Verse*, with Blueprint deprecated once that framework matures.
- **Blueprint costs are structural, not incidental.** Binary `.uasset` assets do not merge - the editor diff works only inside the editor and requires manual node copying, which is why third-party merge tooling exists and why file locking is the industry answer. There is no text search, no review in ordinary tools, no automated refactoring. Nativization was deprecated in 4.27 and removed in 5.0 and is not returning: the generated code was slower than handwritten C++ and harder to debug, and many nodes could not be nativized cleanly. Current Epic guidance is to profile and rewrite hot Blueprints in C++ by hand.
- **The accessibility premise does not hold.** Visual scripting is still programming: the author must still know which of thousands of nodes to use, and debugging, tangled logic and documentation navigation remain. It mainly serves people who can already program.
- **The cost is higher than embedding a text language, not lower.** A graph editor, a node registry driven by reflection, a type system with coercions, a compiler, a VM, a node-level debugger, diff and merge, graph versioning, copy/paste, comments, collapse-to-function and search. Epic has staffed this for over a decade. Godot gave up. Unity stopped investing.

The distinction that explains all of the above: **domain-specific graphs succeed, general-purpose ones fail.** Material and shader graphs, behaviour trees, animation state machines, VFX graphs and dialogue graphs work because the node vocabulary is small and closed, and because the data genuinely is a DAG or an automaton - the graph is the natural representation rather than a picture of imperative code. General-purpose visual scripting is imperative code with worse ergonomics, and it degrades around fifty nodes of moderate connectivity. This is why Godot deleted one and kept the other, and it is why rung 3 above is graphs and rung 4 is text.

### A custom scripting language
Rejected. Parser, type checker, debugger, language server, formatter, documentation and teaching material are a multi-year cost with no differentiation. Godot still pays it for GDScript.

### Rhai
Rejected as the default candidate. Pure-Rust embedding is convenient, but tree-walking execution and thin editor tooling do not compare with a gradually typed language that has a maintained language server.

### LuaJIT
Rejected. No JIT on iOS, and the C API is pinned to Lua 5.1.

### C# hosting
Rejected. Godot demonstrates the second-class-citizen failure mode: marshalling problems, a separate release cycle, incomplete platform coverage. The hosting complexity is also disproportionate to the goal.

### wasm as the gameplay layer
Rejected for first-party gameplay. The linear-memory boundary buys sandboxing we do not need from our own code and costs authoring ergonomics. It remains the right answer for untrusted third-party code, which is a mod-loading question rather than a gameplay-authoring one.

## Why Luau, if a trigger fires
- Gradual typing gives type-aware diagnostics, autocomplete, go-to-definition and rename through `luau-lsp`, plus `luau-lsp analyze` as a CI check on project scripts. Plain Lua offers none of this.
- Sandboxing is built in: `safeenv` isolates script globals from each other and protects builtin libraries from monkey-patching. That is the foundation trigger 3 needs.
- The interpreter is roughly as fast as the LuaJIT interpreter and requires no JIT. iOS forbids runtime code generation and our Android target benefits from the same property. Optional native codegen on x64 and arm64 covers compute-dense scripts.
- `mlua` provides maintained Luau bindings for Rust.
- Precedent at scale: the Hazelight Unreal AngelScript fork exists for this exact problem statement - visual scripting becomes unmaintainable as complexity grows while C++ imposes long iteration - and shipped *It Takes Two* and *Split Fiction* with over 1.7 million lines across more than 16,000 script files. A statically typed, engine-integrated scripting language is proven at production scale.

## Constraints, should this be adopted
- **Scripts are not systems.** Scripts do not iterate archetypes. The exposed surface is observers and event handlers, per-entity behaviour objects with typed component accessors, and batched query calls that cross the boundary once per query rather than once per entity. Simulation-heavy work stays in Rust. Without this rule the FFI boundary consumes the benefit of the archetype storage.
- Unordered table iteration is not exposed on deterministic paths; the API provides explicitly ordered iteration, so ADR 0001 determinism holds.
- One virtual machine per world. A script error is contained to its callback, reported, and does not abort the frame.
- `.d.luau` definitions are generated during the build from the type registry, never maintained by hand.
- The script API falls under the compatibility policy (ADR 0005).
- The engine must build, run and pass its tests with the backend disabled.

## Consequences
- Phases 0-3 carry no scripting work. `script` stays a reserved, empty backend slot so that adopting a VM later does not disturb the crate graph.
- Phase 4 delivers data-driven behaviour, not a script API.
- Because `script` is a replaceable backend, a wasm backend for untrusted mods can be added later without touching gameplay code.
- Script debugger integration, coroutine scheduling and per-script profiler attribution are non-MVP even if this ADR is accepted.
