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

- MSRV is set in `Cargo.toml` (`rust-version`, currently 1.97); keep it compatible.
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

This project uses a CLI ticket system for task management; tickets live in
`.tickets/` as plain markdown. The `tk` CLI ships in the nix devshell (it is
the `wedow/ticket` flake input). Run `tk help` when you need to use it.
When creating a new issue, run `tk` from within the crate directory that the issue is for so that it can get a better prefix.

## Additional behavioral guidelines

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

## Engineering Tenets

These tenets govern this project. They are non-negotiable.

1. **We Build to Endure.** Systems that are difficult to outgrow, difficult to entangle, easy to reason about, easy to measure. Optimize for structural strength, not short-term applause.
2. **Modularity Is Power.** Every subsystem: narrow responsibility, minimal dependency surface, replaceable internals, stable API. Monoliths are a last resort.
3. **Incrementalism Everywhere.** Full rebuilds are failure modes. Deltas over rewrites. Patches over full uploads. Caches over recomputation. Budgeted work over spikes.
4. **Introspection Is Non-Optional.** If we cannot measure it, we cannot improve it. Every system exposes: time, memory, work units, bandwidth. Diagnostics are architecture.
5. **Explicit Over Implicit.** No hidden state. No invisible scheduling. No accidental lifetime behavior. No magical performance characteristics. Predictability is a feature.
6. **Long-Term > Short-Term.** Clean structure over clever shortcuts. Extensibility over demo velocity. Architectural leverage over temporary wins.
7. **Replaceability Is a Constraint.** Major subsystems tolerate different backends, techniques, allocators, platforms. If something cannot be replaced, it must be small and contained.
8. **Calm Interfaces.** Internal complexity may be aggressive. Public APIs must be calm: boring, obvious, stable, intentional.
9. **No Sacred Subsystems.** Refactor without attachment. Remove complexity when possible. Evolve forward.
