# ADR 0006: No code hot reloading in 0.1

- Status: Accepted
- Date: 2026-09-02

## Decision
rgine 0.1 does not hot reload compiled Rust code. Iteration is served by asset, scene and config hot reload, and by fast restart of the isolated play world (ADR 0004).

## Reasons
- Rust has no stable ABI. Dynamic libraries are not guaranteed compatible across compiler versions, and in practice not across separate builds either.
- Implementations that do work impose engine-wide constraints. Our Machinery achieved DLL hot reloading in C only by requiring C interfaces everywhere, modelling objects as a data pointer plus function pointers, collecting all plugin globals into a single `memcpy`-able state struct, and forbidding headers from including other headers. Even then breakpoints do not survive a reload and files stay locked.
- Fyrox, which does support this in Rust, requires `RUSTFLAGS="-C prefer-dynamic=yes"`. Without it the standard library is duplicated between engine and plugin, which produces subtle bugs.
- Rust build times reduce the value of code hot reload even where it works. The mechanism competes with the compile it is meant to avoid.

## Consequences
- The iteration budgets in ROADMAP target asset reload and play world restart, not code reload. They are the contract this ADR is measured against.
- Gameplay iteration that genuinely must avoid a rebuild is served by the scripting layer (ADR 0007), where the ABI is ours to define.
- This decision is revisited only if a measured iteration budget cannot be met by any other means, and the revisit must account for the API constraints listed above.
