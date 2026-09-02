# xtask

Repository automation, run through the alias in `.cargo/config.toml`:

```bash
cargo xtask check-deps
```

## check-deps

Asserts the crate boundaries recorded under "Enforced boundaries" in
`docs/ARCHITECTURE.md`, over the workspace graph reported by `cargo metadata`:

- `editor` and `devtools` are not reachable from any runtime crate. Only
  `[dependencies]` edges count, so a test may still use `devtools`.
- crates under `crates/` do not name a crate under `apps/`, in any table.
- a backend crate is named only by the crate that owns it, per ADR 0009. Every
  table counts, including target-specific ones, since a backend named only for
  Android is still named.

Owners live in `BACKEND_OWNERS` in `src/rules.rs` and are the one place to edit
when a backend lands or moves. The table is validated against the workspace
before it is enforced: an entry naming a crate that does not exist, or an owner
that no longer uses its backend, fails the check rather than quietly passing.

Every violation is reported with the manifest it came from and what to do about
it, and the run reports all of them rather than stopping at the first.
