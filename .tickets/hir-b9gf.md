---
id: hir-b9gf
status: closed
deps: []
links: []
created: 2026-05-22T21:30:33Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-0, foundation]
---
# Phase 0 — Foundation

## Goal

Establish the Rust workspace structure, CI, and project conventions for the
Hirð compiler. Most of this is already in place from the repo scaffold; the
remaining work is splitting `crates/hird` into the compiler's crate topology
and adding project documentation skeletons.

## v0.1 demo relevance

Every subsequent phase builds on the workspace and convention decisions made
here. This phase produces no compiler functionality but is load-bearing for all
of it.

## Tasks

- Split `crates/hird` into the compiler crate topology:
  `hird-lex`, `hird-parse`, `hird-ast`, `hird-types`, `hird-effects`,
  `hird-actors`, `hird-ir`, `hird-codegen`, `hird-cli`.
- Add `hird-mcp` and `hird-lsp` crates (initially empty scaffolds).
- Create `runtime/` directory for the Erlang support library (not a Rust crate).
- Add `CONTRIBUTING.md` skeleton with build, test, and style instructions.
- Add `ARCHITECTURE.md` skeleton mapping crate responsibilities.
- Confirm and document in `DECISIONS.md`:
  - Erlang source as the v0.1 backend target.
  - Rust toolchain MSRV (1.92) and edition (2024).
  - DI-style handlers in v0.1 (no CPS/delimited control).
  - Per-process effect semantics.
  - Opaque-capability discipline for stateful resources.
  - Unicode canonicalization at the lexer.

## Task sequence

1. [x] [hir-8unj](hir-8unj.md) — Scaffold compiler crate topology
2. [ ] [hir-jg95](hir-jg95.md) — Project documentation skeletons

## Out of scope

- No compiler logic in any crate (just `lib.rs` stubs).
- No Erlang code in `runtime/` yet.
- No `phrasebook.md` content beyond skeleton headings.

## Acceptance Criteria

- `cargo fmt --all` passes.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- `cargo test --workspace --all-features` passes (trivially — no real tests yet).
- All compiler crates exist as workspace members with `#![no_std]` and workspace lints.
- `hird-cli` is the exception: it may use `std`.
- `runtime/` directory exists with a placeholder README.
- `CONTRIBUTING.md`, `ARCHITECTURE.md`, and `DECISIONS.md` exist with substantive skeletons.
- CI still green after workspace restructuring.


## Notes

**2026-07-28T07:21:50Z**

Closing as overtaken by events: the foundation this epic exists to
establish has been in place for nine phases. The workspace topology
shipped via hir-8unj and then evolved past this ticket's crate list
(hird-effects removed by ADR-014; hird-check and hird-actors added);
runtime/ holds the real Erlang support library; DECISIONS.md contains
all eight locked decisions named here (ADR-001 through ADR-008) and
fifteen more; fmt/clippy/test conventions are enforced and green.

The one genuinely unfinished piece is hir-jg95: CONTRIBUTING.md and
ARCHITECTURE.md still do not exist (DECISIONS.md long exceeded its
ask). That is an ordinary P2 documentation task, not a foundation
blocker, so it stays open on its own rather than holding a P0 epic
open.
