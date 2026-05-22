---
id: hir-t1cj
status: open
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

