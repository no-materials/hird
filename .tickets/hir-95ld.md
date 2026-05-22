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

