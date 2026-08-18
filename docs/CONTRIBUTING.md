# Contributing

Project AI-powered!

Before submitting changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Architecture changes require an ADR under `docs/adr/`.
