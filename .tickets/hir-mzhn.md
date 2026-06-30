---
id: hir-mzhn
status: closed
deps: []
links: [hir-t1cj]
created: 2026-05-22T21:43:40Z
type: task
priority: 1
assignee: nomaterials
tags: [decision, design, effects, handlers]
---
# OD7: Handler semantics in v0.1

Resolve handler semantics for v0.1.

**Resolution**: DI-style (dependency injection). Not Koka-style resumable handlers.

A handle block in v0.1 provides function implementations for declared effects.
The compiler routes effectful calls through those implementations. This is
conceptually similar to Reader monad injection or manual dependency passing.

What v0.1 handlers CAN do:
- Replace a tool implementation with a mock.
- Redirect log output to a different sink.
- Wrap an effect with audit recording.
- Provide a dry-run implementation.

What v0.1 handlers CANNOT do:
- Resume a computation (no delimited continuations).
- Interleave effects (no cooperative scheduling within a handler).
- Capture the continuation for later replay.

These limitations are acceptable because the v0.1 use case (agent systems with
tool effects) doesn't need resumable handlers. DI-style is sufficient for
mocking, dry-runs, and audit.

Koka-style handlers may be added in v0.2+ via free-monad lowering or selective
CPS transformation, only where it pays off.

**Decision point**: Phase 5 implementation.

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- Handler implementation is DI-style.
- Documentation explicitly states what handlers can and cannot do in v0.1.
- Koka-style handlers documented as future work.


## Notes

**2026-06-30T10:49:32Z**

Resolved by hir-t1cj. OD7 is DI-style handler semantics, documented in ADR-004 (decision, what handlers can/cannot do, Koka-style deferred to v0.2+) and refined by ADR-013 (v0.1 structural checking scope and lowering). Removed from the open-decision-slots table. DI-style handlers are implemented in hird-check/hird-effects/hird-ir.
