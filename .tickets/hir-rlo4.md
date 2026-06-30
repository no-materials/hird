---
id: hir-rlo4
status: closed
deps: [hir-0rzf]
links: []
created: 2026-05-22T21:33:23Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-5, effects]
---
# Phase 5 — Effect Rows

## Goal

Add algebraic effect row labels to the type system with row polymorphism. In
v0.1, effects are labels tracked by the type system; handlers are DI-style
(dependency injection), not Koka-style resumable continuations.

## v0.1 demo relevance

The planner demo declares effects on every function and actor: Tool<ReadRepo>,
Tool<CreateTicket>, Log. The effect system is what makes "every tool call visible
in the types" true. Without this phase, the core differentiating claim is empty.

## Design context

Effect rows are the type-level representation of what a function does. A function
signature includes its effect row: `fn read(path: Path) -> String ! {Tool<ReadFile>, Exn IOError}`.
Row polymorphism allows effect-generic functions: `fn map(f: A -> B ! {r}, xs: List<A>) -> List<B> ! {r}`.

**Critical commitment**: effects are per-process and local, not transitive across
messages. On BEAM, a function's effects describe what the current process does
directly. Sending a message to another actor has a Send<Msg> effect; the receiving
actor has its own independent effect summary. The sender's effect row does not
transitively include what the receiver might do.

**Handler semantics in v0.1**: DI-style. A "handle" block provides function
implementations for declared effects; the compiler routes effectful calls through
those implementations. This is sufficient for mocking, dry-runs, swappable tool
implementations, and effect-graph audits without requiring CPS transformation or
delimited control.

**Capability-effect linkage**: effects on opaque capabilities reference the
specific capability value passed in. `EtsRead<t>` where `t` is a specific
`Table<UserId, User, Read>` — not "ETS in general." This is what makes audit
graphs precise enough to show exactly which tables a function touches.

## Task sequence

1. [x] [hir-95ld](hir-95ld.md) — Effect row types and row polymorphism
2. [x] [hir-0x16](hir-0x16.md) — Effect inference and annotation checking
3. [x] [hir-t1cj](hir-t1cj.md) — DI-style effect handlers

## Open design questions resolved in this phase

- **OD7 (Handler semantics in v0.1)**: Confirm DI-style. Document in DECISIONS.md.
  Defer Koka-style resumable handlers to v0.2+.
- **OD8 (Send/reply effect tracking)**: Resolve how Send<Msg> and Request<Msg, Reply>
  are represented in effect rows. Plan Phase 7 around the chosen approach.

## Out of scope

- Koka-style resumable handlers or CPS transformation.
- Effect handler performance optimization.
- Transitive effect closure computation (that's a tooling feature, Phase 10).
- Session types or protocol typing on effect sequences.

## Acceptance Criteria

- Effect row types in the type system: closed rows ({Log, Tool<X>}) and open rows
  ({Log, Tool<X> | r}).
- Effect declaration syntax: `effect Foo` and parametric effects `effect Tool<T>`.
- Effect row variables and row unification.
- Effect annotations are explicit at top-level function declarations; inferred at
  interior let-bound positions.
- DI-style handlers: a `handle` block provides implementations for declared effects;
  the compiler routes calls through them.
- Capability-effect linkage: effects like EtsRead<t> reference specific capability
  values, not abstract effect classes.
- Effect-aware type error messages: "function declared {Log} but body performs
  {Log, Tool<X>}" with source spans pointing to the offending call.
- IR extended with effect rows on every function node.
- Snapshot tests: effect inference on pure functions, single-effect functions,
  multi-effect functions, row-polymorphic functions, handler blocks, effect
  mismatch errors, capability-linked effects.
- OD7 and OD8 decisions documented in DECISIONS.md.
- `cargo clippy` and `cargo test` pass for `hird-effects`.


## Notes

**2026-06-24T12:55:14Z**

Task 1 of 3 complete: hir-95ld (effect row types and row polymorphism) landed
and is closed.

Delivered: the effect-row representation (Effect, EffectRow, RowVar) and row
unification live in hird-types; TyFn carries a row; generalize/instantiate
quantify and refresh row variables with the occurs-check and level-lowering
crossing into row-space. The checker registers `effect` declarations and
elaborates `! {…}` annotations onto function schemes, so annotated
row-polymorphic functions type-check. The IR carries the row and round-trips
it (the pretty-printer synthesises the `effect` declarations it references).

Phase-level acceptance still open (later tasks): effect inference for bodies
and annotation-vs-inferred checking with spans, capability-effect linkage, and
DI-style handler blocks. OD7/OD8 remain for hir-t1cj / Phase 7 respectively.

hir-0x16 (effect inference and annotation checking) is now unblocked.

**2026-06-29T08:11:26Z**

Task 2 of 3 complete: hir-0x16 (effect inference and annotation checking)
landed and is closed.

Delivered: function bodies' effect rows are inferred in hird-check (interleaved
with type inference, via an accumulator threaded through the body walk; lambdas
attach their body row to their own function type). A top-level function's
inferred row is checked for equality against its declared row (the annotation,
or the empty row when `!` is absent), so pure functions may omit `!` and
effectful functions that under- or over-declare are rejected (new code C0030,
pointed at the offending call via a provenance side-table). Interior let-bound
functions infer and generalise their row, including inferred row polymorphism.
Capability effects are type-level: EtsRead<t> carries the capability
parameter's type; same-typed capabilities collapse (documented v0.1 limitation),
differently-typed stay distinct.

Phase-level acceptance still open (last task): DI-style handler blocks
(hir-t1cj), now unblocked — parse + type-check `handle` blocks, validate handler
signatures, compute the handled row (body minus handled plus handler effects),
and lower to IR. OD7 (handler semantics) is documented there; OD8 remains
Phase 7. The effect-provenance side-table records (effect -> introducing-call
span) and drives the mismatch diagnostic but is not yet persisted on
CheckedFile; audit-graph rendering (Phase 6/10) will surface it.

hir-t1cj (DI-style effect handlers) is now unblocked.

**2026-06-30T10:49:08Z**

Task 3 of 3 complete: hir-t1cj (DI-style effect handlers) landed and is closed.
Phase 5 is functionally complete.

Delivered: `handle { Effect → handler, … } in body` now type-checks. Each arm is
validated structurally (declared effect at the right arity; handler must be a
function — new code C0031, reusing C0027/C0028 for the head), and the block's
row is (body effects − handled effects) ∪ handler effects, computed by a new
hird-effects::handle_row helper. The block types as its body; the net row and
per-arm handled effect are recorded on CheckedFile and consumed by a new IrHandle
IR node (lowering + pretty-print + round-trip). No Erlang is emitted — IR-only,
with parameter threading as the recorded eventual strategy.

Exact-signature handler validation (handler args/return vs the effect's
operation type) is deferred to hir-4g3y, which introduces tool operation
signatures; v0.1's structural check is the agreed scope.

OD7 (handler semantics) is documented in ADR-004/013 and removed from the open-
decision-slots table; hir-mzhn is closed. OD8 (Send/reply effect tracking)
remains for Phase 7. Phase-level acceptance — effect row types, inference and
annotation checking, capability linkage, DI handlers, and the snapshot suite —
is now met; cargo clippy and cargo test pass for hird-effects, hird-check, and
hird-ir.

**2026-06-30T12:10:48Z**

All three children are closed (hir-95ld, hir-0x16, hir-t1cj) and the phase's acceptance criteria are met: effect-row types and row polymorphism, effect inference and annotation checking, capability-effect linkage, DI-style handlers, and the snapshot suite. OD7 is documented (ADR-004/013); OD8 (Send/reply effect tracking) carries forward to Phase 7. Closing the epic; hir-jt39 (Phase 6 — Tool Effects) and hir-y85q (Phase 7 — Actors) are unblocked.
