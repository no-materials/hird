---
id: hir-fbze
status: closed
deps: []
links: [hir-0bhk]
created: 2026-05-22T21:42:55Z
type: task
priority: 1
assignee: nomaterials
tags: [decision, design, errors]
---
# OD1: Crash vs error boundary design

Resolve the crash-vs-error boundary for Hirð.

**Context**: OTP says "let it crash." Effect rows say "errors are values."
Hirð must draw a clear, compiler-enforced line between the two.

**Proposed resolution**:
- Domain errors are Exn effects: Exn ParseError, Exn HttpError, etc.
  Handled with pattern matching or effect handlers. Do not kill the process.
- Crashes are resource failures and explicit panics: crash!("msg").
  Propagate as Erlang exits. Caught by supervisors. Cannot be caught in
  normal Hirð code (only supervisors handle them).
- crash! is a divergent function (return type: Never / !).
- A function's effect row tells you what domain errors it can produce.
  The possibility of crashing is NOT in the effect row — it's implicit in
  any function that does I/O or calls crash!.

**Alternatives considered**:
1. Crash as an effect (Crash in the effect row) — rejected because it would
   appear on nearly every function, providing no useful information.
2. No crash! primitive (only Exn effects) — rejected because it prevents
   fast failure on truly unrecoverable situations.
3. Crash as a result type (Result<T, Crash>) — rejected because crashes are
   not values; they bypass normal control flow.

**Decision point**: Phase 8 implementation. Must be decided before supervisor
codegen and error-model documentation.

**Dependencies**: blocks Phase 8 completion.

## Acceptance Criteria

- Written decision in DECISIONS.md with context, decision, alternatives, consequences.
- docs/error-model.md reflects the decision with examples.
- Compiler implements the chosen semantics.


## Notes

**2026-07-10T06:33:23Z**

Resolved. The decision is locked as ADR-021 in DECISIONS.md (context, decision,
alternatives, consequences); docs/error-model.md reflects it with examples; and
the compiler implements the chosen semantics — domain errors are Exn values in
the effect row, crashes are the divergent crash!/panic! primitive typed
∀a.(String)→a with an empty row, propagating as Erlang exits to the supervisor.
