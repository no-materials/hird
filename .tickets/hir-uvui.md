---
id: hir-uvui
status: open
deps: [hir-4g3y]
links: []
created: 2026-07-01T13:47:08Z
type: task
priority: 2
assignee: nomaterials
parent: hir-jt39
tags: [phase-6, effects, handlers, tools]
---
# Signature-directed handler checking for tool effects

Now that tool declarations introduce operation signatures, upgrade DI-style
handle-arm checking from structural to signature-directed for tool effects:
validate that a handler expression's argument and result types match the
handled tool's operation signature (`{args} -> result`), not merely that the
head is a declared effect at the right arity and the handler is a function.

This closes the handler-signature-validation gap that ADR-013 deferred until
tool declarations introduced those signatures; ADR-015 now provides them.

## Acceptance Criteria

- A handle arm's handler is validated against the handled tool's operation
  signature; a type mismatch produces a compile error.
- The existing structural checks are retained: unknown effect, wrong arity,
  and non-function handler.
- Snapshot tests: a matching handler that type-checks, and a mismatched
  handler that is rejected.
