---
id: hir-0x16
status: open
deps: [hir-95ld]
links: []
created: 2026-05-22T21:38:53Z
type: task
priority: 1
assignee: nomaterials
parent: hir-rlo4
tags: [phase-5, effects, inference]
---
# Effect inference and annotation checking

Implement effect inference and checking: infer effect rows for function bodies,
check declared annotations against inferred rows, and produce clear error
messages on mismatches.

**Effect inference rules**:
- Pure expressions (literals, variables, let bindings of pure values) have
  empty effect row {}.
- Function application: if f : A -> B ! {E}, then f(x) has effects {E}.
- Effectful primitives (send, spawn, tool calls) have their declared effects.
- Sequential composition: effects union. If e1 has {E1} and e2 has {E2},
  then (e1; e2) has {E1 ∪ E2}.
- Let bindings: `let x = e1 in e2` has the union of e1's and e2's effects.
- Match expressions: scrutinee effects union with the join of all arm effects.
- Lambda: the lambda itself is pure; its body's effects are part of the
  lambda's *function type*, not the lambda expression's effects.

**Annotation checking**:
- Top-level function declarations must have explicit effect annotations.
- Interior let-bound functions have inferred effects.
- If a function is declared `fn foo(x: Int) -> Int ! {Log}` but its body
  performs {Log, Tool<X>}, that's a type error: "function declared {Log} but
  body performs {Log, Tool<X>}."
- The error should point to the call site that introduces the undeclared effect.

**Capability-effect linkage**:
- When a function takes a capability parameter `t: Table<K, V, Read>` and
  performs `lookup(t, key)`, the effect is `EtsRead<t>` — linked to the
  specific capability value, not just the effect name.
- This requires the effect system to track which capability values flow into
  which effect-producing operations.

## Acceptance Criteria

- Effect inference produces correct effect rows for: pure functions, single-effect
  functions, multi-effect functions, sequential composition, nested lets, match arms.
- Top-level functions require explicit effect annotations; missing annotation is error.
- Annotation mismatch error: "declared {Log} but body performs {Log, Tool<X>}" with
  span pointing to the Tool<X> call site.
- Capability-linked effects: EtsRead<t> where t is a specific parameter.
- Row-polymorphic functions infer correctly: map(f, xs) where f has effects {r}
  gives map effects {r}.
- Snapshot tests: pure function, single effect, multi-effect, mismatch error,
  row-polymorphic inference, capability-linked effect, nested function effects.
- At least 15 snapshot tests.

