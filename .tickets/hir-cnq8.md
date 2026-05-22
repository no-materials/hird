---
id: hir-cnq8
status: open
deps: [hir-y85q]
links: []
created: 2026-05-22T21:34:27Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-8, supervision, otp]
---
# Phase 8 — Supervision

## Goal

Bring OTP supervision trees into the type system: supervisor declarations with
typed child specs, restart strategies, and a clear boundary between domain
errors (effect-typed values) and crashes (untyped, propagate to supervisor).

## v0.1 demo relevance

The planner demo runs under a supervisor with a declared one_for_one restart
strategy. The supervisor type-checks that its child specs match the Planner
actor's init requirements. The error-vs-crash distinction is visible in the
demo: a ParseError from malformed tool output is an effect-typed value returned
to the caller; a network timeout that kills the process propagates to the
supervisor for restart.

## Design context

Supervisor declarations:
```
supervisor PlannerSup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, start: Planner.start_link, restart: permanent }
  ]
}
```

The type system validates:
- Child references resolve to declared actors.
- Init configs match child actors' expected start arguments.
- Restart strategies are valid enum values.

**The error-vs-crash boundary** (OD1) is this phase's central design decision:

- **Domain errors** are values carried by effect rows. A function that might fail
  with a parse error has `Exn ParseError` in its effect row. The caller handles it
  with normal pattern matching or effect handling. Domain errors do not kill the
  process.

- **Crashes** are resource failures, panics, and unrecoverable bugs. They propagate
  as Erlang exits and reach the supervisor. The language provides a `panic!`-equivalent
  (or `crash!`) that explicitly crosses from domain-error space into crash space.

The language must make it syntactically obvious which class of error code can
produce. A function with only `Exn` effects cannot crash (barring bugs). A
function that calls `crash!` or performs I/O that might fail at the resource
level has that visible in its type or its calling context.

## Open design question resolved in this phase

- **OD1 (Crash vs error boundary)**: Resolve the exact mechanism. Document in
  DECISIONS.md and `docs/error-model.md`.

## Out of scope

- Dynamic supervision (adding children at runtime with typed specs — future work).
- Supervision across nodes (distributed OTP).
- rest_for_one and one_for_all strategies (implement one_for_one first; add others
  as follow-up tickets if time permits, but they are not required for the demo).
- Application-level OTP structure (application behaviors, release generation).

## Acceptance Criteria

- `supervisor` declaration syntax parsed and type-checked.
- Child specs validated: child IDs resolve to actor declarations, start function
  types match, restart strategy is a valid enum.
- Type-system distinction between domain errors (Exn effects) and crashes
  (process exits).
- `crash!` or equivalent panic primitive exists with clear semantics: crosses
  from error space to crash space, reaches supervisor.
- Supervisor codegen produces Erlang supervisor behavior modules.
- `docs/error-model.md` written: explains error-vs-crash boundary, when to use
  each, how they interact with effect rows and supervision, with code examples.
- OD1 decision documented in DECISIONS.md.
- Snapshot tests: supervisor declarations, child spec validation errors,
  crash! semantics, error-vs-crash type-level distinction.
- `cargo clippy` and `cargo test` pass.

