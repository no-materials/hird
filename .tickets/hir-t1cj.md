---
id: hir-t1cj
status: closed
deps: [hir-0x16]
links: [hir-mzhn]
created: 2026-05-22T21:39:14Z
type: task
priority: 1
assignee: nomaterials
parent: hir-rlo4
tags: [phase-5, effects, handlers]
---
# DI-style effect handlers

Implement v0.1 effect handlers as dependency injection: a handle block provides
function implementations for declared effects, and the compiler routes effectful
calls through those implementations.

**Surface syntax**:
```
handle {
  Log.info  -> fn(msg) { io:format("~s~n", [msg]) },
  Tool<ReadRepo> -> fn(args) { mock_read_repo(args) },
} in {
  planner_main(repo_path)
}
```

**Semantics**:
- A handle block introduces a scope where named effects are bound to handler
  implementations.
- Within the handled scope, calls to effectful operations route through the
  handler function instead of the default implementation.
- Handlers must match the expected signature of the effect they handle:
  if Tool<ReadRepo> expects { path: Path } -> RepoState, the handler must
  accept { path: Path } and return RepoState.
- Handler type mismatches are compile errors.
- Unhandled effects pass through (the handle block's own effect row is the
  body's effects minus the handled effects plus any effects the handlers
  themselves introduce).

**Lowering**:
- Handle blocks lower to Erlang as function-parameter threading or process
  dictionary lookup (design choice — flag for implementation).
- The simplest approach: lower handled effects to an extra function parameter
  (a handler map or record) threaded through calls. This avoids process
  dictionary mutation but increases arity.
- Alternative: store handlers in process dictionary, look up at call site.
  Simpler codegen but less pure.

**Use cases this enables in v0.1**:
- Mock handlers for testing (replace Tool<ReadRepo> with a mock).
- Dry-run execution (replace Tool<CreateTicket> with a logger).
- Log redirection (replace Log with a custom sink).
- Audit interception (wrap a handler to record invocations).

This ticket resolves **OD7 (Handler semantics in v0.1)**: confirm DI-style,
document in DECISIONS.md.

## Acceptance Criteria

- handle block syntax parsed and type-checked.
- Handler signature validated against effect declaration.
- Handler type mismatches produce compile errors.
- Effect row of handle block correctly computed: body effects minus handled
  effects plus handler effects.
- Lowering to IR: handle blocks produce IR nodes with handler bindings.
- At least one lowering strategy implemented (parameter threading or proc dict).
- OD7 documented in DECISIONS.md.
- Snapshot tests: valid handler, handler type mismatch, nested handlers,
  effect subtraction in handle block's row, handler introducing new effects.
- At least 8 snapshot tests.


## Notes

**2026-06-30T10:13:25Z**

Locked the two open implementation decisions (recorded as ADR-013).

Decision 1 — Handler checking is structural in v0.1. A handle arm checks iff its
head is a declared effect at the correct arity and the handler expression has a
function type. Exact handler-signature validation (handler args/return vs the
effect's operation type) is deferred to hir-4g3y (tool declarations), which is
what introduces operation signatures. This narrows the "Handler signature
validated against effect declaration" / "Handler type mismatches produce compile
errors" criteria to: unknown effect, wrong arity, and non-function handler.

Decision 2 — Lower to IR only. Add an IR handle node carrying the handler
bindings and body; emit no Erlang (hird-codegen is a stub; backend is later per
ADR-002). Parameter threading is the chosen eventual Erlang strategy (explicit,
no hidden state); process-dictionary lookup rejected. "At least one lowering
strategy implemented" is satisfied at the IR level.

Surface syntax follows phrasebook + ADR-009: whole-effect handler arms
(`Log -> fn(...) ...`, not `Log.info -> ...`) and a bare expression after `in`
(no `{ }` block). The ticket's `Log.info` and `in { ... }` examples are
illustrative only, not the implemented grammar.

**2026-06-30T10:48:45Z**

Implemented and verified.

Parsing and AST were already in place (handle/in keywords, HANDLE_EXPR/
HANDLE_ARM, HandleBlock/HandleArm); added a HandleArm::effect() accessor for the
effect head.

Type-checking (hird-check): infer_handle infers the body into a fresh effect
accumulator (mirroring lambda), validates each arm structurally per ADR-013 —
the head must be a declared effect at the right arity (reuses C0027/C0028) and
the handler must have a function type (new C0031) — and computes the block's row
as (body − handled) ∪ handler effects. The block types as its body. The net row
and each arm's handled effect are recorded on CheckedFile (effect_rows /
handled_effects) for the IR.

Row algebra (hird-effects): the crate gains handle_row(body, handled, handler),
the pure (body − handled ∪ handler) computation ADR-013 places here; hird-check
now depends on hird-effects.

IR: new IrHandle/IrHandleArm carrying the arms (handled effect + handler), body,
computed row, and result type. Lowering reads the checker side-tables; the
pretty-printer re-emits `handle { Effect → handler, … } in body` and synthesises
effect declarations from handle arms (collect_expr_effects), so a handled effect
named only in an arm still re-checks. No Erlang emitted (IR-only per ADR-013;
parameter threading is the recorded eventual strategy).

OD7: documented in ADR-004 (DI-style) and ADR-013 (v0.1 checking scope +
lowering); removed from the open-decision-slots table.

Tests: 8 checker inference snapshots (subtraction, unhandled remainder, handler-
introduced effect, multi-arm, nested, unknown effect, arity, non-function
handler), 3 IR snapshots/round-trips (pretty-print, JSON, structural) plus 2
round-trip property tests, 5 hird-effects unit tests, and a HandleArm::effect()
AST assertion. cargo fmt, clippy -D warnings, and test --workspace all pass.

**2026-06-30T12:07:37Z**

Follow-up refactor (post-close): the handled-row helper was a single ~15-line pure function over EffectRow/Effect and did not justify its own crate. Moved hird_effects::handle_row into hird-types as a free function beside EffectRow/unify_row/resolve_row (where the rest of the row algebra lives), and removed the hird-effects crate entirely — including its unused entries in the workspace and in hird-cli/hird-actors. Recorded as ADR-014 (supersedes the crate-placement clauses of ADR-011 §1 / ADR-012 §1 and ADR-013's lowering-placement consequence); future handler lowering will live with codegen. fmt, clippy -D warnings, and test --workspace all pass.
