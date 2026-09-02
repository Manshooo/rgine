<!--
The PR title becomes the squash commit message, so it must be a valid
conventional commit line: <type>(<scope>): <imperative summary>
-->

Closes #

## What and why

<!-- What changed, and the reason. The diff shows the what; this is for the why. -->

## How this was verified

<!-- CI green is enough for a refactor. Rendering, performance or determinism
     changes need numbers, a screenshot, or the test that now covers it. -->

## Checklist

- [ ] CI is green
- [ ] New public items are documented
- [ ] An ADR was added or referenced, if this touched architecture
- [ ] `CHANGELOG.md` updated under `Unreleased`, if this is visible to an engine user
- [ ] Persisted format change ships with its migration and a round-trip test (ADR 0002, ADR 0005)
- [ ] `BREAKING CHANGE:` footer present, if anything broke
- [ ] Iteration budgets in `ROADMAP.md` still hold
