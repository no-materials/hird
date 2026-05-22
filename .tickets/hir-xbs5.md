---
id: hir-xbs5
status: open
deps: [hir-1dvq]
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

