---
id: hir-95ld
status: open
deps: [hir-a6lz]
links: []
created: 2026-05-22T21:38:37Z
type: task
priority: 1
assignee: nomaterials
parent: hir-rlo4
tags: [phase-5, effects, types]
---
# Effect row types and row polymorphism

Extend the type system with effect row types and row polymorphism.

**Effect row representation in hird-effects**:
- EffectRow::Closed(BTreeSet<Effect>) — a fixed set of effects: {Log, Tool<X>}.
- EffectRow::Open(BTreeSet<Effect>, RowVar) — an open row: {Log, Tool<X> | r}.
- EffectRow::Empty — the empty effect row {} (pure).
- Effect::Named(Name) — a simple named effect: Log, Spawn.
- Effect::Parametric(Name, Vec<Type>) — a parametric effect: Tool<ReadRepo>,
  EtsRead<Table<K,V,Read>>, Send<Msg>.

**Effect declarations in surface syntax**:
- `effect Log` — declare a simple effect.
- `effect Tool<T>` — declare a parametric effect.
- `effect EtsRead<T>` — declare a capability-linked effect.

**Row polymorphism**:
- Row variables behave like type variables but range over effect rows.
- Row unification: unifying {Log | r} with {Log, Tool<X>} binds r to {Tool<X>}.
- Row unification: unifying {Log | r1} with {Tool<X> | r2} produces
  r1 = {Tool<X> | r3}, r2 = {Log | r3} (row variable splitting).
- Row variable occurs check (prevents infinite effect rows).

**Integration with function types**:
- Function types carry an effect row: TyFn(A, B, EffectRow).
- Pure functions have the empty row: A -> B ! {}.
- The ! {} is elided in display for pure functions.

**Extend IR**: IrFnDef now carries a non-empty effect row.

## Acceptance Criteria

- EffectRow and Effect types defined in hird-effects.
- effect declarations parsed and registered in type environment.
- Row variables allocated and managed by substitution table.
- Row unification: closed-closed, open-closed, open-open cases all handled.
- Row variable splitting works correctly.
- Function types carry effect rows; pure functions have empty row.
- IR IrFnDef carries effect row.
- Unit tests: row unification cases (at least 10), effect declaration parsing,
  parametric effects.
- Snapshot tests for effect row display: {Log}, {Log, Tool<X>}, {Log | r}, {}.


## Notes

**2026-06-24T09:54:55Z**

DESIGN DECISIONS (locked 2026-06-24; supersede conflicting points in this
ticket's body/ACs). Binding architecture record: DECISIONS.md ADR-011.

Crate placement (D1 — overrides "EffectRow and Effect types defined in
hird-effects"):
- Effect, EffectRow, RowVar, and row unification live in hird-types (TyFn
  carries the row; Subst manages row vars; row-unify is mutually recursive
  with type-unify, so they cannot be split across the hird-effects ->
  hird-types dependency edge). hird-effects hosts effect inference (hir-0x16)
  and handler lowering (hir-t1cj). hird-ir replaces its placeholder
  `struct EffectRow {}` with the hird-types type.

Row variables (D2): a separate row union-find inside Subst, indexed by a
distinct RowVar newtype, sharing the single binding-level counter. Cross-kind
binding is impossible by construction (compile error, not a runtime assert).
generalize/instantiate extended to row vars; quantified vars carry their kind
(TyForall today holds only Vec<u32> type vars). Rejected alternative: a single
kinded union-find over a Type-or-Row term.

EffectRow shape (D3 — overrides the 3-variant enum + BTreeSet<Effect>):
- One struct: { effects: BTreeMap<Name, Vec<Effect>>, tail: Option<RowVar> }.
  Closed = tail None; open = tail Some; empty = empty map + None. Idempotent
  set semantics ({Log,Log}={Log}).
- Do NOT key/dedup by a structural Ord over Effect: Parametric carries
  unifiable Types, so Ord is unstable under substitution (solving a var
  corrupts ordering/equality). Outer key = effect-constructor Name
  (substitution-stable); multiple effects per head coexist in the value Vec
  (Tool<ReadRepo> + Tool<CreateTicket>). Equality/dedup compare RESOLVED args.
- Row unification: match by head, unify same-head effects' type args via the
  existing type unify(); open/open splits the residual into a fresh tail row
  var; row occurs-check on tails. Implement parametric-arg unification now
  (cheap, correct, one hook into type unify). Do NOT build
  scoped-labels/multiset machinery (Koka v0.2, deferred per ADR-004/ADR-011).

Correctness checklist (binding):
- Level-lowering AND occurs-check must cross type->row: descend through every
  TyFn row and through Effect::Parametric type args, else generalize()
  over-quantifies a row var and an effect escapes its handler. Add a direct
  escape test.
- Prove row unification terminates (decreasing measure on residuals). Write
  the open/open overlapping-head idempotence test FIRST — get it wrong and
  generalize loops forever.
- TyFn gains a row field -> ~50 sites across 7 files default to the empty row;
  land as one sweep certified GREEN by the IR round-trip property test, THEN
  extend round-trip + pretty-printer to cover NON-empty rows.
- Row-unify failures must carry structure (offending effect, expected vs
  actual residual) so hir-0x16 can render good diagnostics (Introspection
  tenet).

Scope boundary vs hir-0x16 (D4):
- THIS ticket: EffectRow/Effect/RowVar + row unification (all cases,
  splitting, occurs); TyFn carries rows; generalize/instantiate over rows;
  elaborate `! {...}` annotations (currently parsed-but-ignored by hird-check)
  into EffectRow; register `effect` declarations; populate IrFnDef.effect_row
  from annotations; pretty-print + round-trip. Annotated row-polymorphic
  functions (e.g. map with !{r}) must type-check here.
- hir-0x16: infer rows for bodies/interior lets; check annotated-vs-inferred;
  mismatch diagnostics with spans; capability-effect linkage (EtsRead<t> ->
  specific value).

OD8 (D5): Send<Msg>/Request<Msg,Reply>/Await<Reply> semantics stay Phase 7
(DECISIONS OD table), NOT this phase — the epic body's "OD8 resolved in this
phase" is superseded. hir-95ld's only obligation is that Effect::Parametric is
expressive enough to represent those later; no special-casing now.

Starting-state note: lexer (! token, effect/handle keywords) and parser
(parse_effect_decl, parse_effect_ann) already produce CST nodes; hird-check
currently parses-but-ignores them; hird-ir already has the placeholder
EffectRow + IrFnDef.effect_row field. So the remaining work is the
type-theoretic core, annotation elaboration, and wiring — not surface syntax.
