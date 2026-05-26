---
id: hir-rlo4
status: open
deps: [hir-0rzf]
links: []
created: 2026-05-22T21:33:23Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-5, effects]
---
# Phase 5 — Effect Rows

## Goal

Add algebraic effect row labels to the type system with row polymorphism. In
v0.1, effects are labels tracked by the type system; handlers are DI-style
(dependency injection), not Koka-style resumable continuations.

## v0.1 demo relevance

The planner demo declares effects on every function and actor: Tool<ReadRepo>,
Tool<CreateTicket>, Log. The effect system is what makes "every tool call visible
in the types" true. Without this phase, the core differentiating claim is empty.

## Design context

Effect rows are the type-level representation of what a function does. A function
signature includes its effect row: `fn read(path: Path) -> String ! {Tool<ReadFile>, Exn IOError}`.
Row polymorphism allows effect-generic functions: `fn map(f: A -> B ! {r}, xs: List<A>) -> List<B> ! {r}`.

**Critical commitment**: effects are per-process and local, not transitive across
messages. On BEAM, a function's effects describe what the current process does
directly. Sending a message to another actor has a Send<Msg> effect; the receiving
actor has its own independent effect summary. The sender's effect row does not
transitively include what the receiver might do.

**Handler semantics in v0.1**: DI-style. A "handle" block provides function
implementations for declared effects; the compiler routes effectful calls through
those implementations. This is sufficient for mocking, dry-runs, swappable tool
implementations, and effect-graph audits without requiring CPS transformation or
delimited control.

**Capability-effect linkage**: effects on opaque capabilities reference the
specific capability value passed in. `EtsRead<t>` where `t` is a specific
`Table<UserId, User, Read>` — not "ETS in general." This is what makes audit
graphs precise enough to show exactly which tables a function touches.

## Task sequence

1. [ ] [hir-95ld](hir-95ld.md) — Effect row types and row polymorphism
2. [ ] [hir-0x16](hir-0x16.md) — Effect inference and annotation checking
3. [ ] [hir-t1cj](hir-t1cj.md) — DI-style effect handlers

## Open design questions resolved in this phase

- **OD7 (Handler semantics in v0.1)**: Confirm DI-style. Document in DECISIONS.md.
  Defer Koka-style resumable handlers to v0.2+.
- **OD8 (Send/reply effect tracking)**: Resolve how Send<Msg> and Request<Msg, Reply>
  are represented in effect rows. Plan Phase 7 around the chosen approach.

## Out of scope

- Koka-style resumable handlers or CPS transformation.
- Effect handler performance optimization.
- Transitive effect closure computation (that's a tooling feature, Phase 10).
- Session types or protocol typing on effect sequences.

## Acceptance Criteria

- Effect row types in the type system: closed rows ({Log, Tool<X>}) and open rows
  ({Log, Tool<X> | r}).
- Effect declaration syntax: `effect Foo` and parametric effects `effect Tool<T>`.
- Effect row variables and row unification.
- Effect annotations are explicit at top-level function declarations; inferred at
  interior let-bound positions.
- DI-style handlers: a `handle` block provides implementations for declared effects;
  the compiler routes calls through them.
- Capability-effect linkage: effects like EtsRead<t> reference specific capability
  values, not abstract effect classes.
- Effect-aware type error messages: "function declared {Log} but body performs
  {Log, Tool<X>}" with source spans pointing to the offending call.
- IR extended with effect rows on every function node.
- Snapshot tests: effect inference on pure functions, single-effect functions,
  multi-effect functions, row-polymorphic functions, handler blocks, effect
  mismatch errors, capability-linked effects.
- OD7 and OD8 decisions documented in DECISIONS.md.
- `cargo clippy` and `cargo test` pass for `hird-effects`.

