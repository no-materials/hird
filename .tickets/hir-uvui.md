---
id: hir-uvui
status: closed
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

## Notes

**2026-07-06T13:49:44Z**

Implementation decisions (agreed after review of ADR-013/015 and the current checker):

- Generic tools: check the handler by instantiating the tool's signature with
  fresh type variables and unifying. This accepts a monomorphic handler for a
  generic tool (e.g. an LLMCall handler fixed at Schema<Int>) — an accepted,
  documented v0.1 gap; requiring a polymorphic handler needs skolemisation /
  polymorphic subsumption the checker doesn't have. Record as a one-liner in
  DECISIONS.md.
- Signature lookup: add a small tool-signature side-table on the checker keyed
  by marker name (beside invocation_records), rather than looking up the
  generated function name in the value env (indirect, user-shadowable).
- Tool<X> where X is not a declared tool: report an error ("not a declared
  tool") rather than silently falling back to the structural check.
- Diagnostics ordering: keep C0031 (non-function handler) as the structural
  pre-check; add a new code for signature mismatch so a non-function handler
  doesn't surface as a confusing unification error.
- Rows are not unified: the handler is checked against (args) -> result with a
  fresh open effect row — a mock may be pure and need not carry the tool's
  declared trailing row (e.g. Exn ParseError). Handler effects join the block's
  row as today.
- Non-tool effects (bare labels) keep the existing structural path untouched.

**2026-07-06T14:07:16Z**

Implemented in commit 6da2ac0. Handle arms over Tool<Marker> are checked against the tool's operation signature per the locked decisions: side-table lookup keyed by marker name, instantiate-and-unify (monomorphic handler for a generic tool accepted), fresh open row (pure mocks accepted), C0033 for non-tool markers, C0034 for signature mismatch after the structural C0031 pre-check. ADR-017 records the decision. Snapshot tests cover the matching, mismatched, generic-monomorphic, non-tool-marker, and non-function cases. fmt/clippy/test clean across the workspace.
