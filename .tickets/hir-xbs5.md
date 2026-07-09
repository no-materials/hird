---
id: hir-xbs5
status: open
deps: []
links: []
created: 2026-05-22T21:40:39Z
type: task
priority: 1
assignee: nomaterials
parent: hir-cnq8
tags: [phase-8, supervision]
---
# Supervisor declarations and type validation

Implement supervisor declarations with type-level child spec validation.

**Supervisor declaration syntax**:
```
supervisor PlannerSup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: default_config(), restart: permanent },
  ]
}
```

**Type-level validation**:
- Child references resolve to declared actors.
- start_args type matches the child actor's init function parameter type.
- restart strategy is a valid enum: permanent, temporary, transient.
- intensity and period are positive integers.
- Child IDs are unique within a supervisor.
- Duplicate child ID is a compile error.

**Restart strategies for v0.1**: one_for_one only. one_for_all and rest_for_one
are syntactically valid but produce a "not yet implemented" warning pointing to
a future ticket. The demo only needs one_for_one.

**Supervisor effect summary**:
- A supervisor's effect row is the union of its children's effect rows.
- This is computed automatically from child actor declarations.
- The supervisor's own effect summary is purely derived, not declared.

## Acceptance Criteria

- supervisor declaration syntax parsed and type-checked.
- Child refs resolve to actor declarations; unresolved refs are compile errors.
- start_args type-checked against child actor init signature.
- restart strategy validated (permanent/temporary/transient).
- intensity and period validated as positive integers.
- Duplicate child IDs are compile errors.
- one_for_one codegen (next ticket); one_for_all and rest_for_one produce warnings.
- Supervisor effect row computed from children.
- IR includes supervisor nodes.
- Snapshot tests: valid supervisor, unresolved child, type mismatch on start_args,
  duplicate child ID, supervisor effect summary.
- At least 6 snapshot tests.

## Decisions locked (v0.1)

**Init arity — supervised actors take exactly one init parameter.** A child
spec's `start_args` is a single expression, type-checked against the child
actor's sole init parameter type. An actor whose init has ≠1 parameters cannot
be a supervised child (compile error); bundle multiple fields into one tuple or
record parameter. Matches OTP's `start_link/1` convention; relaxing to
positional multi-arg later is additive. `spawn` stays variadic and is
unaffected.

**`start_args` must be pure.** It is evaluated during supervisor init, so its
effect row must be empty. Effectful start arguments are a compile error in
v0.1, and start_args therefore contributes nothing to the derived effect row.

**Supervisor body fields are a closed set.** Exactly `strategy`, `intensity`,
`period`, `children` — each required, each at most once. Unknown or duplicate
fields are compile errors. No implicit defaults: `intensity` and `period` are
always written explicitly.

**Child spec fields are a closed set.** Exactly `id`, `actor`, `start_args`,
`restart` — each required, each at most once. Unknown or duplicate fields are
compile errors. `id` is a bare lowercase identifier, unique within the
supervisor; `actor` resolves in the actor namespace.

**Children are workers, one level deep.** Every child is an actor, lowered with
`type => worker`. A supervisor cannot itself be a child of another supervisor
in v0.1 (no declaration-level nested trees).

**Children are module-local.** `actor` references resolve within the current
module; cross-module children are not expressible (actors are module-local).

**Empty `children` is permitted.** A supervisor may declare zero children.

**Effect row is derived, never declared.** A supervisor has no trailing
effect-summary syntax; its effect row is computed as the union of its child
actors' per-actor effect summaries.

**Unsupported-strategy warning references nothing.** `one_for_all` and
`rest_for_one` parse and type-check but emit a warning that they are not
implemented in v0.1 (only `one_for_one` is). Per the repository rule, the
warning text names no ticket, ADR, or phase. This is the checker's first
`Severity::Warning` and needs a fresh `CheckCode`.

Additional snapshot targets from the above: ≠1-parameter init under
supervision, effectful `start_args`, unknown/duplicate field, and the
unsupported-strategy warning.

