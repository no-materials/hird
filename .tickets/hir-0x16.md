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


## Notes

**2026-06-29T06:51:23Z**

DESIGN DECISIONS (locked 2026-06-29; supersede conflicting points in this
ticket's body/ACs). Architecture records: DECISIONS.md ADR-012 (which
supersedes the crate-placement clause of ADR-011 §1 and refines ADR-006).

A. INFERENCE PLACEMENT (ADR-012 §1). Effect inference lives in hird-check,
   interleaved with type inference — NOT in hird-effects. An effect accumulator
   is threaded through the body walk: application unions the callee's RESOLVED
   row into the enclosing function's row; let/sequence/match union their parts;
   a lambda is pure as an expression and its body effects attach to the lambda's
   function-type row (accumulator resets at each lambda boundary). hird-effects
   keeps handler lowering (hir-t1cj) only. This intentionally deviates from the
   original "inference lives in hird-effects" placement.

B. CAPABILITY-EFFECT LINKAGE (ADR-012 §2). Type-level + provenance, NOT
   value-level identity in the type layer.
   - EtsRead<t> elaborates with the capability parameter's TYPE as the effect
     argument (EtsRead<Table<UserId,User,Read>>); call sites instantiate via the
     existing type unify() — no new machinery, no new Type variant, no
     value-into-effect substitution dimension.
   - Binding-site identity + the introducing call's span are recorded in a
     PROVENANCE side-table during inference, separate from the effect row. This
     is the SAME side-table item C below needs for diagnostics, so capability
     linkage is not extra scope.
   - Two capabilities of the SAME type collapse to one row element (idempotent
     set, ADR-011) — expected and faithful for v0.1 (planner caps Tool<ReadRepo>,
     Tool<CreateTicket>, Log are distinctly typed). Encode the same-typed-merge
     as a PASSING test documenting the limitation, not a TODO.
   - Rejected: per-binding skolem/singleton identities (option b) — a
     non-generalisable identity flowing through instantiation/row-arg unify is
     unsound under polymorphism (two call sites alias one resource node) or
     becomes option (c) in disguise; it perturbs the just-stabilised
     generalisation/level-lowering core and would split this ticket. Rejected:
     extending the row with a value dimension (option c) — perturbs ADR-011 row
     equality/unification, the riskiest code we own.
   - Deferred to v0.2+ (additive): true per-value distinctness via singleton
     capability identities or value-indexed effect args; refines the existing
     effect-arg slot, forcing no row-representation change.

C. ANNOTATION CHECKING = EQUALITY via row unification (not subsumption). The
   body's inferred row must UNIFY with the function's declared row; a mismatch
   reuses the row-unify failure structure from hir-95ld (offending effect,
   expected/got residual) and the provenance map supplies the span at the
   offending call. Equality (not "declared is an upper bound") is chosen because
   it keeps the audit graph honest (no phantom declared-but-unused effects) and
   reuses the existing, terminating row unification rather than a new subsumption
   relation. Row-polymorphic declared rows still work: the open-row tail variable
   absorbs the residual through ordinary row unification (apply/map !{r}
   unchanged). Subsumption/upper-bound contracts are a possible v0.2 ergonomic
   refinement, explicitly out of scope here.

D. TOP-LEVEL vs INTERIOR rows.
   - Top-level functions: the effect row is DECLARED — the `! {…}` annotation, or
     the empty row {} when `!` is absent — and the inferred body row is checked
     against it (per C). So a pure top-level fn may omit `!`; an effectful one
     that omits/under-declares its effects fails the equality check. This is how
     "top-level effects are explicit" is enforced — it falls out of the check, no
     separate "missing annotation" error needed, and no `! {}` boilerplate on
     pure functions.
   - Interior let-bound functions and lambdas: the row is INFERRED from the body,
     attached to the TyFn row, and generalised alongside type variables (the
     generalise/instantiate-over-row-vars + occurs + level-lowering machinery
     from hir-95ld). Not checked against any declaration.

E. PROVENANCE SIDE-TABLE (shared infrastructure). One structure, recorded during
   inference, mapping each introduced effect to (introducing-call span,
   capability binder/argument). It serves BOTH the annotation-mismatch diagnostic
   ("declared {Log} but body performs {Log, Tool<X>}", span on the Tool<X> call)
   AND capability-to-resource linkage; audit-graph rendering (Phase 6/10) consumes
   it later and is out of scope here.

AC ADJUSTMENTS (supersede the body's ACs where they conflict):
- Drop "EtsRead<t> where t is a specific parameter VALUE". Reframe to: capability
  effects carry the capability's TYPE, plus binding-site provenance. Same-typed
  capabilities merge in the row (documented, tested).
- Keep ≥15 snapshot tests. Required cases: pure fn; single effect; multi-effect;
  sequential/nested let union; match scrutinee ∪ arm-join; lambda body-row on the
  function type (not the enclosing row); row-polymorphic inference (map/apply
  !{r}); capability effect carries the capability type (EtsRead<Table<…>>); two
  DIFFERENTLY-typed capabilities give two distinct effects; two SAME-typed
  capabilities collapse to one (limitation-as-passing-test); annotation-vs-
  inferred MISMATCH with span at the offending call; pure top-level fn omitting
  `!` is accepted; effectful top-level fn under-declaring is rejected.

SCOPE BOUNDARY: hir-0x16 RECORDS provenance and proves effect kinds; it does not
render audit graphs (Phase 6/10) and does not build handler lowering (hir-t1cj).
