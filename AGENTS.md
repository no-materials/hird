## Forest Engineering Tenets

These tenets govern all forest-rs projects. They are non-negotiable.

1. **We Build to Endure.** Systems that are difficult to outgrow, difficult to entangle, easy to reason about, easy to measure. Optimize for structural strength, not short-term applause.
2. **Modularity Is Power.** Every subsystem: narrow responsibility, minimal dependency surface, replaceable internals, stable API. Monoliths are a last resort.
3. **Incrementalism Everywhere.** Full rebuilds are failure modes. Deltas over rewrites. Patches over full uploads. Caches over recomputation. Budgeted work over spikes.
4. **Introspection Is Non-Optional.** If we cannot measure it, we cannot improve it. Every system exposes: time (CPU + GPU), memory (live + fragmentation), work units, bandwidth. Diagnostics are architecture.
5. **Explicit Over Implicit.** No hidden state. No invisible scheduling. No accidental lifetime behavior. No magical performance characteristics. Predictability is a feature.
6. **Long-Term > Short-Term.** Clean structure over clever shortcuts. Extensibility over demo velocity. Architectural leverage over temporary wins.
7. **Replaceability Is a Constraint.** Major subsystems tolerate different backends, techniques, allocators, platforms. If something cannot be replaced, it must be small and contained.
8. **Calm Interfaces.** Internal complexity may be aggressive. Public APIs must be calm: boring, obvious, stable, intentional.
9. **No Sacred Subsystems.** Refactor without attachment. Remove complexity when possible. Evolve forward.

# AGENTS.md

This repository is maintained with help from AI coding agents (e.g. Codex/ChatGPT/Claude).
This file defines how to make changes, what “done” means, and the project defaults we enforce.

## North Star

- Keep core crates small, predictable, and long-lived.
- Prefer simple, explicit designs over clever ones.
- Avoid dependency creep; keep compile times and surface area under control.
- Optimize for long-term architecture over short-term compatibility; it’s OK to break callers to get the right core shape.

## Non-negotiables (Definition of Done)

- `cargo fmt` passes.
- `cargo clippy` passes (`-D warnings`).
- Public APIs are documented (types/functions; public fields/variants where it matters).
- Tests updated/added when behavior changes.
- Examples/benchmarks live in separate top-level workspace crates (no extra dev-deps in core crates).
- Never reference tickets, ADRs, plans, phases, or agent sessions in code,
  comments, docstrings, or commit messages. These are project-management
  artifacts that rot. Code should justify itself; context belongs in the
  commit message or PR description, not in the source tree.

Suggested commands:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Before implementing a ticket

Before writing any code, read these files in order:

1. The assigned ticket (`tk show <id>`).
2. `DECISIONS.md` — locked architecture constraints you must not violate.
3. `phrasebook.md` — canonical surface syntax conventions.
4. The parent epic ticket if one exists (for broader context and out-of-scope boundaries).

## Rust workspace expectations

- MSRV is set in `Cargo.toml` (`rust-version = "1.92"`); keep it compatible.
- Follow workspace lint policy (notably: `unsafe_code = "deny"` and `missing_docs = "warn"`).

## `no_std` policy (core crates)

- Default assumption for foundational crates: `#![no_std]` whenever practical (use `extern crate alloc` when needed).
- Keep `std` behind an explicit `std` feature flag when required.
- Avoid `std` collections in `no_std` crates; use `hashbrown` (and `alloc` types) instead.

## Tests, examples, benchmarks

- Unit tests live next to code; keep them deterministic.
- Examples and benchmarks live in separate top-level workspace crates so extra dependencies don’t appear as dev-dependencies of core crates.

## Tooling / workflow

- Prefer `rg` for code search.
- Keep diffs small and reviewable; preserve existing style unless improving consistency.
- Wrap markdown prose to a readable source width when creating or editing docs;
  avoid huge single-line paragraphs that are hard to review in diffs.

## Tickets / Issue Tracking / Plans

This project uses a CLI ticket system for task management. Run `tk help` when you need to use it.
When creating a new issue, run `tk` from within the crate directory that the issue is for so that it can get a better prefix.
