---
id: hir-a6lz
status: open
deps: [hir-ee8k]
links: []
created: 2026-05-22T21:38:22Z
type: task
priority: 2
assignee: nomaterials
parent: hir-0rzf
tags: [phase-4, ir, testing]
---
# IR pretty-printer and round-trip tests

Implement the Glass IR pretty-printer and the round-trip property test.

**Pretty-printer**:
- Takes IR and emits Hirð source code in canonical form.
- Output is syntactically valid and parseable.
- Indentation and formatting follow canonical style (to be defined).
- Type annotations are included on all top-level declarations.
- Effect annotations are included where present (empty in pre-Phase 5 IR).

**Round-trip property**:
The core correctness property: for any well-typed program,
  source -> parse -> infer -> lower_to_IR -> pretty_print -> parse -> infer -> lower_to_IR
produces an IR that is structurally equivalent to the first IR (modulo source spans).

This property test catches:
- Pretty-printer bugs (output doesn't parse).
- Lowering bugs (information lost in lowering).
- Inference instability (re-inference produces different types).

Implement as:
1. A set of hand-written programs that exercise all IR node kinds.
2. A proptest generator for well-typed programs (reuse from Phase 3 if available).
3. The round-trip assertion as a reusable test harness.

This property becomes the regression safety net for all future compiler changes.

## Acceptance Criteria

- Pretty-printer emits valid, parseable Hirð source from IR.
- Round-trip property holds for at least 10 hand-written programs.
- proptest round-trip runs for randomly generated programs (at least 100 cases).
- Pretty-printer output matches canonical formatting style.
- Snapshot tests for pretty-printer output on representative programs.

