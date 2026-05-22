---
id: hir-jj3l
status: open
deps: [hir-of12]
links: []
created: 2026-05-22T21:37:14Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, types]
---
# Type representation and unification engine

Build the core type representation and unification engine in hird-types.

**Type representation** (the Type enum):
- TyVar(u32) — unification variable, identified by index.
- TyCon(Name, Vec<Type>) — type constructor applied to arguments.
  Includes built-in constructors: Int, Float, String, Bool, List, Option.
- TyFn(Box<Type>, Box<Type>) — function type A -> B.
- TyTuple(Vec<Type>) — tuple type (A, B, C).
- TyRecord(BTreeMap<Label, Type>) — structural record type { x: A, y: B }.
- TyForall(Vec<TyVar>, Box<Type>) — quantified type (after generalization).

Type variables are managed by a substitution table (union-find or equivalent).
Fresh variables are allocated from a counter.

**Unification**:
- Unify two types, producing a substitution or an error.
- Occurs check to prevent infinite types (TyVar appearing in its own solution).
- Unification of function types, tuple types, record types (structural), and
  applied constructors.
- Row unification is NOT in this ticket (that comes with effect rows in Phase 5).
  But the unification engine should be designed to be extensible for rows.

**Error reporting**:
- Unification failures produce TypeMismatch { expected: Type, got: Type, span: Span }.
- OccursCheck errors produce InfiniteType { var: TyVar, in_type: Type, span: Span }.
- Type display: types render to human-readable strings (Int, String, A -> B,
  List<Int>, (Int, String), { name: String, age: Int }).

## Acceptance Criteria

- Type enum defined with all variants listed.
- Substitution table with union-find or equivalent, O(α(n)) lookup.
- Unification of all type kinds: variables, constructors, functions, tuples, records.
- Occurs check catches and reports infinite types.
- Type display renders readable type strings.
- Error types carry source spans.
- Unit tests: unify Int with Int (success), Int with String (failure), A with Int
  (binds A), (A -> B) with (Int -> String) (binds both), occurs check triggers.
- At least 15 unit tests covering the unification engine.

