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

- `cargo fmt --all -- --check`
- `cargo check --workspace --locked` on Linux, Windows, macOS and the Android target
- `cargo test --workspace --locked`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo check --workspace --all-targets --locked` on the MSRV toolchain
- `cargo deny check all`
- `cargo xtask check-deps`

As the corresponding roadmap phases land, these join the list: the public API and schema snapshot checks required by ADR 0005, and the iteration budget harness.

## Task tracking

GitHub Issues. One issue per unit of work, and every pull request closes exactly one.

**Milestones are roadmap phases.** `Phase 1. ECS + App Loop`, `Phase 2. Render v1`, and so on. An issue with no milestone is backlog - that is what backlog means here, not a separate list.

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

## Dependencies and toolchain

### Minimum supported Rust version

MSRV is `1.85`, recorded once in `[workspace.package].rust-version` and checked by the `msrv` CI job. It is what edition 2024 requires, so it cannot drop below that.

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
