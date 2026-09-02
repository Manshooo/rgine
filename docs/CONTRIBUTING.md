# Contributing

Project AI-powered!

Before submitting changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Architecture changes require an ADR under `docs/adr/`.

## Workflow

- Branch from `master`, named `<type>/<slug>`: `feat/`, `fix/`, `refactor/`, `docs/`, `chore/`.
- Commit messages are [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), scoped by crate: `refactor(platform): ...`.
- One pull request per change, squash merged.
- User-visible changes get a `CHANGELOG.md` entry under `## [Unreleased]`.
