---
id: hir-n3si
status: closed
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


## Notes

**2026-06-16T13:10:09Z**

Locked decisions for implementation. These resolve the open points in the
ticket text against the constraints already locked in hir-lhyh (D1-D9) and
DECISIONS.md; each takes the default recommended in review.

**L1 — Or-patterns are out of scope, not a deferred stretch goal.** There is
no OR_PAT anywhere in the pipeline: the lexer/parser/AST have no
pattern-level alternation, and `|` (PIPE) is ADT-constructor-list syntax
only. Supporting `Some(1) | Some(2) ->` would need new grammar, parser, and
AST-projection work that does not belong in a type-checking ticket. Drop
or-patterns from this ticket; the "Or-patterns (stretch goal)" line is
superseded. File a separate parser/AST ticket if they are wanted later —
exhaustiveness over them is then purely additive (an or-pattern lowers to
multiple matrix rows).

**L2 — Follow D6: Bool is an ADT; there are no Bool literals.** The ticket's
literal-pattern example `true` and its "Bool" literal line are superseded by
D6 (locked in hir-lhyh): Bool is the seeded ADT `True | False`, a lowercase
`true` parses as a catch-all BIND_PAT, and the lexer has no true/false
tokens. Literal patterns are Int/Float/String only. Bool exhaustiveness needs
no special-casing — it falls out of the standard ADT machinery via the seeded
registry entry. Do not add bool literals.

**L3 — One uniform rule for open (non-finite) signatures.** Exhaustiveness is
driven by the resolved scrutinee's head. A head is *closed* only when it is a
registered finite ADT (user-declared or the seeded Bool) or a tuple (a single
constructor). Every other head — Int/Float/String, an unresolved type
variable, a record, a function type, or any type constructor with no ADT
entry — has an *open* signature: the match is exhaustive iff some arm is a
wildcard or variable catch-all. Missing-constructor enumeration ("missing:
None, Nil") applies only to closed ADTs; a non-exhaustive open match reports
that a catch-all `_` arm is required rather than naming constructors. This
also disposes of the undeclared-List/Option case: a constructor pattern on an
undeclared type already fails C0007 upstream, so the only way to reach an open
head is an all-wildcard match, which is trivially exhaustive.

**L4 — Redundancy diagnostic is the redundant arm's span only.** Emit the
AC-required warning carrying the span of the redundant (unreachable) arm with
an "unreachable arm" message. The ticket's illustrative "already covered by
pattern on line N" back-reference is out of scope for v0.1: pinpointing the
subsuming arm needs subsumption bookkeeping beyond the usefulness predicate,
and the AC only requires the redundant-arm span.

**Placement / codes.** Lives in hird-check (per D1; no new crate). Two new
diagnostic codes, continuing the existing C-series (highest is C0014): C0015
non-exhaustive match (error — non-negotiable per the epic), C0016 redundant
pattern (warning).
