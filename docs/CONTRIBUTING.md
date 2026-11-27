# Contributing

Project AI-powered!

The workflow itself is recorded in [ADR 0008](adr/0008-development-workflow.md); this document is the operational reference for it.

## Before submitting changes

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Architecture changes require an ADR under `docs/adr/`.

## Branches

`master` is the only long-lived branch. It must build at every commit.

Work happens on short-lived branches named:

```
<type>/<issue>-<slug>
```

`<type>` is one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `spike`. `<issue>` is the GitHub issue number and may be omitted for chores that do not warrant one. `<slug>` is kebab-case and describes the change, not the file touched.

```
feat/42-ecs-archetype-storage
fix/117-winit-resize-panic
chore/gitattributes
spike/render-graph-prototype
```

One issue, one branch, one pull request. A branch is deleted once its pull request is merged.

`release/0.x` branches exist only when a patch has to ship on top of an existing tag while `master` has already moved on. Do not create one speculatively.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/), with the crate as the scope:

```
<type>(<scope>): <imperative summary>

<body: why this change, not what the diff shows>

Closes #42
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `build`, `ci`, `revert`.

Scopes: `core`, `ecs`, `platform`, `render`, `physics`, `audio`, `asset`, `scene`, `script`, `ui`, `devtools`, `app`, `cli`, `editor`, `launcher`, `xtask`, `deps`, `docs`. Omit the scope when a change genuinely spans the workspace.

A breaking change carries a `BREAKING CHANGE:` footer naming what broke and how to migrate. This is what the release process reads to decide the version bump, so it is not optional.

## Pull requests

Every change reaches `master` through a pull request. There are no exceptions for small changes: the cost of the rule is a few seconds, and the cost of exceptions is an unreviewable history.

- **The title is the merge commit.** Squash merging uses the pull request title, so the title must itself be a valid conventional commit line.
- **Link the issue** with `Closes #N` in the description.
- **Say how it was verified.** "CI green" is enough for a refactor; a rendering or performance change needs numbers or a screenshot.
- **Open as draft** while the work is in progress. A non-draft pull request is a request to merge.
- **Keep it reviewable.** Prefer under roughly 400 changed lines. Beyond that, split by concern - a mechanical rename and a behaviour change do not belong in one pull request.

Self-merge is allowed while the project has one maintainer. Merging with red CI is not, in any circumstance.

The branch ruleset that enforces this lives in `.github/rulesets/master-protect.json`. GitHub does not read it from the repository, so it is applied with:

```bash
gh api -X PUT repos/Manshooo/rgine/rulesets/<id> --input .github/rulesets/master-protect.json
```

Keep the file and the live ruleset in step; the file is the reviewable record of what the trunk is protected by.

## Merging

**Squash merge only.** Merge commits and rebase merges are disabled.

The reason is `git bisect`. One commit per unit of work means every commit on `master` builds and runs, so a performance or behaviour regression can be found by binary search over history. For an engine this is not a stylistic preference - it is the primary tool for the class of bug that is hardest to find any other way.

Required before merge:

- Build the workspace libraries, binaries and examples with all features on Linux, Windows and Android arm64.
- Check workspace libraries, binaries and examples with all features on the MSRV toolchain.
- `cargo deny check all`
- `cargo xtask check-deps`

The CI workflow adds Intel and Apple Silicon macOS builds for release tags and manual runs. Use `gh workflow run ci.yml --ref master` to build on all platforms. Pull requests restore the Rust cache, and runs on `master` save it. Run tests, formatting, Clippy and the relevant performance checks locally.

## Task tracking

GitHub Issues. One issue per unit of work, and every pull request closes exactly one.

**Roadmap milestones track phase exits.** `Phase 1. ECS + App Loop`, `Phase 2. Render v1`, and so on. The `Render follow-ups` milestone groups deferred rendering work. Issues without a milestone remain in the backlog.

**Labels:**

| Prefix | Values |
| --- | --- |
| `area:` | one per crate: `area:ecs`, `area:render`, `area:asset`, ... plus `area:ci`, `area:docs` |
| `type:` | `type:feat`, `type:bug`, `type:perf`, `type:docs`, `type:chore`, `type:spike` |
| priority | `P0` blocks the current phase, `P1` planned for it, `P2` desirable |
| state | `blocked`, `needs-adr` |

Labels and milestones are created by `./.github/setup-repo.sh`. It is idempotent and is the record of the taxonomy - add labels there rather than by hand in the web interface, or the two drift apart.

**Ready to start** means the issue states what and why, has acceptance criteria that can be checked, and carries an area label and a milestone. An issue that needs an architectural decision gets `needs-adr` and does not start until the ADR is accepted.

`PROJECT_STATUS.md` is updated at phase boundaries, not per pull request. Updating it on every merge produces constant conflicts and adds nothing the issue tracker does not already show.

## Architecture decisions

ADRs live in `docs/adr/NNNN-kebab-title.md`, numbered monotonically. Numbers are never reused or renumbered.

Status is one of `Proposed`, `Accepted`, `Rejected`, or `Superseded by ADR NNNN`.

An accepted ADR is not rewritten. A decision that changes gets a new ADR that supersedes the old one, and the old one is marked. Typo and link fixes are fine; changing the substance of a recorded decision in place destroys the record of why the project is shaped the way it is.

An ADR is required to add or remove a crate, change a boundary between crates, pick the dependency behind a replaceable backend, change a persisted format, or add a public subsystem. When in doubt, write one - they are cheap.

## Versioning and releases

All workspace crates share one version and ship together. They are not independently useful, and versioning them separately multiplies the compatibility matrix that has to be tested.

The compatibility policy is [ADR 0005](adr/0005-compatibility-policy.md). Before 1.0, cargo treats the second position as the compatibility unit, so the three levels in that ADR map onto two positions:

| ADR 0005 level | Pre-1.0 | From 1.0 |
| --- | --- | --- |
| breaking | `0.1.4` -> `0.2.0` | major |
| additive | `0.1.4` -> `0.1.5` | minor |
| no API or schema change | `0.1.4` -> `0.1.5` | patch |

The changelog distinguishes additive from fix-only releases even where the version number cannot.

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/) and is edited **in the pull request that makes the change**, under `## [Unreleased]`. Writing the changelog at release time means reconstructing it from commit messages, which is where inaccuracies come from.

Release:

1. Confirm `master` is green.
2. Decide the bump from the `BREAKING CHANGE:` footers and changelog entries since the last tag.
3. Set the workspace version in the root `Cargo.toml` - both `[workspace.package].version` and the version on each engine entry in `[workspace.dependencies]`, which must match it - then run `cargo check --workspace` to refresh `Cargo.lock`.
4. Move `## [Unreleased]` to a dated version section in `CHANGELOG.md`.
5. Merge that as `chore(release): v0.1.0`.
6. Tag `v0.1.0` on the resulting commit and push the tag.
7. Wait for all CI jobs on the tag, including macOS, to pass before publishing the release or distributing its artifacts.

## Dependencies and toolchain

### Minimum supported Rust version

MSRV is `1.89`, recorded in `[workspace.package].rust-version` and checked by the `msrv` CI job. It is the minimum required by the current Rapier dependency graph (ADR 0026).

Raising it is a deliberate `build(deps):` pull request that says which feature or dependency forced the bump - not a side effect of using something new. `rust-toolchain.toml` pins `stable` for day-to-day work; the two are different questions, and the MSRV job overrides the pin with `RUSTUP_TOOLCHAIN` so that it actually tests the older compiler.

### Dependency policy

The policy lives in `deny.toml` and is enforced by the `deny` CI job, which runs `cargo deny check all`: advisories, licences, bans and sources.

- **Licences are permissive only.** The allow-list in `deny.toml` is the authority. The engine is linked into games whose licensing we do not control, so anything copyleft - including file-scoped MPL - is a decision made in a pull request that adds it to the list, with the reason in the description.
- **Advisories are fatal.** An ignored advisory is an entry in `deny.toml` carrying its id, the date and why it does not apply here; the ignore list is reviewed when the entry's cause is fixed upstream.
- **Sources are crates.io only.** A git dependency needs an ADR: it is a fork or an unreleased crate, and both are commitments.
- **Duplicate versions warn rather than fail** while the graph is small enough that the report would be noise.

Adding a dependency is a design decision, not a convenience. Say in the pull request what it does that the standard library and the existing graph do not, and check its licence, MSRV, maintenance and platform coverage - Android included, since a crate that does not build there costs a target. A dependency behind a replaceable backend (`render`, `physics`, `audio`, `platform`, `script`) additionally requires an ADR, per the rules above.

`cargo deny` is not part of the toolchain file; install it once with `cargo install cargo-deny --locked` to run the check locally.

## Definition of done

A change is done when:

- CI is green;
- new public items are documented;
- an ADR was added or referenced if the change touched architecture;
- `CHANGELOG.md` has an entry under `Unreleased` if the change is visible to a user of the engine;
- a persisted format change ships with its migration and a round-trip test against a fixture from the previous schema version (ADR 0002, ADR 0005);
- a performance-sensitive change reports its numbers, and the iteration budgets in `ROADMAP.md` still hold.

## Local setup notes

Line endings are governed by `.gitattributes`: LF in the repository and in the working tree on every platform, regardless of local `core.autocrlf`. Do not override it.

`Cargo.lock` is committed. The workspace produces binaries, and CI builds with `--locked`, so an unlocked dependency graph would make builds non-reproducible. Dependency bumps are a deliberate `build(deps):` pull request, not a side effect of someone's local build.

## Scene merge driver

Install the CLI, then register the driver in each checkout. The command below uses the installed `engine-cli` on your PATH.

```sh
cargo install --path crates/engine-cli --locked
git config --local merge.rgine.name "rgine scene merge"
git config --local merge.rgine.driver 'engine-cli merge %O %A %B %P'
```

The repository assigns this driver to `*.scene.ron` and `*.prefab.ron`. A merge combines edits to separate fields or nodes and checks the resulting parent hierarchy. Each property is compared as a whole RON value, including structured properties such as prefab placements.

A conflict reports the object UUID and affected field when available. Git retains the common, current and incoming versions in its index, and the working-tree file retains the current version. Inspect the inputs with `git show ':1:<path>'`, `git show ':2:<path>'` and `git show ':3:<path>'`. Edit the working-tree RON and stage the result with `git add -- <path>`.

See the [Git merge driver documentation](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver) for the driver placeholders.

## Migrate a project

Run `engine-cli migrate` with the project directory and a new output directory outside the project. The parent of the output directory must already exist.

```sh
engine-cli migrate ./my-project ./my-project-v2
```

The command validates the manifest and authored scene and prefab files before writing a copy. A failure names the source or destination path and leaves the original project intact. The copy includes project files and asset sidecars. The checkout's root `.git` entry stays with the original.

Run the command against a migrated project to verify that it is up to date. This returns success without creating a copy. See [ADR 0021](adr/0021-authored-scene-migration.md) for the supported scene versions.

## Draw a screen-space canvas

```sh
cargo run --locked -p engine-app --example canvas_overlay
```

Resize the window to see the canvas stay centered over the 3D cube. The example builds translucent panels, a rotated texture and lines clipped by two intersecting rectangles. `RUST_LOG=engine_render::drawable=debug` reports `canvas_draw_calls` for presented frames. [ADR 0030](adr/0030-screen-space-canvas.md) defines the coordinates, blending and batching.

Run the native pixel fixture with `cargo test --locked -p engine-render canvas_batches_preserve_order_clipping_and_scale_over_a_3d_scene -- --ignored`. On Windows, set `WGPU_BACKEND` to `dx12` to select Direct3D 12 for the offscreen test.

## Try Deflection Arena

```sh
cargo run --locked -p engine-app --features physics --example deflection_arena
```

Use WASD or the arrow keys to move the disc. When an incoming ball enters the central ring, press Space to strike. A strike lasts eight fixed steps. It uses the held movement direction or aims at the nearest incoming ball. A timed contact inside the ring scores a hit, and the ball turns green.

To exercise the modal input context, hold S while pressing Esc. The disc and balls freeze and Resume stays selected. Release and press S again to select Restart. W selects Resume. Space or Enter confirms and Esc closes the menu. A movement key held through the transition takes effect after a fresh press.

```sh
cargo test --locked -p engine-app --features physics --example deflection_arena
```

The tests verify a physical rebound, an early strike, the pause and restart controls, and replay of recorded input in the same fixed schedule.

### Reload Deflection Arena tuning

Copy `crates/app/examples/deflection_arena/tuning.ron` to `arena-dev.ron` and pass the copy to the example.

```sh
cargo run --locked -p engine-app --features physics,hot-reload --example deflection_arena -- arena-dev.ron
```

- Edit `move_speed` and hold a movement key to see the new speed on the next fixed step after reload.
- Edit `strike_speed_curve` and press Space. Speeds are in world units per second. The curve interpolates linearly between points with strictly increasing `at` values from 0 to 1 over a strike.
- Edit `strike_to_cooldown_ticks` or `cooldown_to_ready_ticks` and start a new strike. The `STRIKE`, `COOLDOWN` and `READY` indicators show the transitions. In-progress strikes keep their scheduled transition ticks.

The terminal reports when it applies an edit. Try a negative `move_speed` to see the source path and validation error. The arena continues with the last valid tuning and applies a later correction. Use the same RON settings when comparing input replays.

## Reload an authored prop during development

Copy the sample scene and the glTF fixture with its external buffer to a working directory. Run the example with the paths to those copies.

```sh
cargo run --locked -p engine-app --features hot-reload --example hot_reload -- ./level.scene.ron ./marker.gltf
```

The sample scene is in [the app fixtures](../crates/app/examples/fixtures/hot-reload.scene.ron). The model and its buffer are in [the asset fixtures](../crates/asset/tests/fixtures). Edit the model, buffer or scene while the window is open. Supply a KTX2 file as the optional third argument to watch the base-color texture. [ADR 0023](adr/0023-development-asset-reload.md) describes the refresh.

Append `--shaders ./crates/render/src/scene` to watch the scene WGSL files. Edit the fragment in `primitive_classic.wgsl` when running with `RGINE_FORCE_CLASSIC=1`, or edit `primitive.wgsl` to change lighting in either binding mode. The app reports shader errors on stderr and keeps drawing with its last working pipelines. [ADR 0024](adr/0024-development-shader-reload.md) describes the pipeline update.

To measure asset, scene and shader reload through the first changed frame on Windows, run in an interactive desktop session and keep the sample window visible.

```powershell
cargo build --locked -p engine-app --features hot-reload --example hot_reload
$env:WGPU_BACKEND = 'dx12'
$env:RGINE_FORCE_CLASSIC = '1'
python .github/scripts/check-hot-reload.py target/debug/examples/hot_reload.exe
```

On Ubuntu, install `xvfb`, `xauth`, `mesa-vulkan-drivers` and `libxkbcommon-x11-0` and run:

```sh
cargo build --locked -p engine-app --features hot-reload --example hot_reload
WGPU_BACKEND=vulkan RGINE_FORCE_CLASSIC=1 xvfb-run -a -s '-screen 0 320x240x24' \
  python3 .github/scripts/check-hot-reload.py target/debug/examples/hot_reload
```
