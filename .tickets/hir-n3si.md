---
id: hir-n3si
status: open
deps: [hir-lhyh]
links: []
created: 2026-05-22T21:37:42Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, types, patterns]
---
# Pattern match exhaustiveness checking

Implement exhaustiveness and usefulness checking for pattern match expressions.

This is non-negotiable for Hirð: actor message handlers use pattern matching on
sum types, and a missing case is a correctness bug that must be caught at compile
time.

**Algorithm**: implement the standard exhaustiveness checking algorithm (Maranget's
"Warnings for Pattern Matching" or equivalent). For each match expression:
1. Collect the constructors of the scrutinee's type.
2. Check that every constructor is covered by at least one pattern.
3. Report missing constructors as compile errors (not warnings).
4. Report redundant patterns as warnings.

**Pattern kinds to support**:
- Constructor patterns: Some(x), Cons(head, tail), None, Nil.
- Variable patterns: x (matches anything, binds).
- Wildcard pattern: _ (matches anything, doesn't bind).
- Literal patterns: 1, "hello", true.
- Tuple patterns: (x, y, z).
- Nested patterns: Some(Cons(x, _)).
- Or-patterns (stretch goal — defer if complex): Some(1) | Some(2) -> ...

**Pattern binding**:
- Variables bound in patterns are available in the match arm body.
- Variable types are inferred from the pattern's position in the type.
- Duplicate variable names in a single pattern are errors.

**Error messages for missing cases**:
- "Non-exhaustive match: missing constructors: None, Nil"
- "Redundant pattern: this case is already covered by pattern on line 5"

## Acceptance Criteria

- Missing constructors produce compile errors listing exactly which are unhandled.
- Redundant patterns produce warnings with span pointing to the redundant arm.
- Nested patterns work: match on Option<List<Int>> handles Some(Cons(x, _)) etc.
- Wildcard and variable patterns are correctly treated as catch-alls.
- Literal patterns checked against their type.
- Pattern-bound variables have correct types in arm bodies.
- Snapshot tests for: complete match (passes), missing constructor (error),
  redundant arm (warning), nested pattern, wildcard, literal pattern.
- At least 12 snapshot tests.

