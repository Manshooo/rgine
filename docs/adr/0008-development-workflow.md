# ADR 0008: Development workflow

- Status: Accepted
- Date: 2026-09-02

## Decision
Trunk-based development on `master`, with every change arriving through a short-lived branch and a squash-merged pull request that CI has passed.

- Branches: `<type>/<issue>-<slug>`, one issue per branch, deleted on merge.
- Commits: Conventional Commits with the crate as scope; `BREAKING CHANGE:` footers are what the release process reads.
- Merge: squash only. Merge commits and rebase merges are disabled.
- Tracking: GitHub Issues. Milestones are roadmap phases; an issue with no milestone is backlog.
- Versioning: one version for the whole workspace, all crates released together, tagged `vX.Y.Z`.
- `CHANGELOG.md` in Keep a Changelog format, written in the pull request rather than at release time.

The operational detail - label taxonomy, pull request expectations, release steps, definition of done - lives in `docs/CONTRIBUTING.md`, which is expected to change more often than this decision.

## Reasons
- **Squash merge exists for `git bisect`.** One commit per unit of work means every commit on `master` builds, so a regression can be found by binary search over history. For an engine, that is the primary tool for performance and behaviour regressions, which are the bugs least likely to be caught by a test that already exists.
- **No exceptions for small changes.** Exceptions are how a history becomes unreviewable, and the rule costs seconds. Self-merge is permitted, so the rule does not block a single maintainer.
- **One workspace version.** The crates are not independently useful. Versioning them separately multiplies the combinations that would have to be tested against each other for no gain.
- **Changelog written at the time of the change.** Reconstructing it from commit messages at release time is where inaccuracy enters, and ADR 0005 makes the changelog load-bearing for deciding the version bump.
- **Milestones mirror roadmap phases** so that the plan and the tracker cannot drift apart. `ROADMAP.md` stays the source of truth for what a phase contains; issues are how it gets executed.
- **`PROJECT_STATUS.md` updates at phase boundaries only.** Per-pull-request updates generate conflicts on a file that duplicates what the tracker already shows.

## Consequences
- Branch protection on `master` is required for this to be real: squash-only merges, required status checks, and no direct pushes. The rules are unenforced until that is configured in the repository settings.
- The required-checks list grows as roadmap phases land: the `xtask` dependency-graph check, `cargo deny`, the ADR 0005 public API and schema snapshots, and the iteration budget harness. Each is added to branch protection in the pull request that introduces it.
- `Cargo.lock` is committed and CI builds with `--locked`, so dependency updates become explicit `build(deps):` pull requests.
- Line endings are pinned by `.gitattributes` rather than by contributor configuration.
- Numbered ADRs are immutable once accepted; a changed decision is a new ADR that supersedes the old one.
