# Architecture Decision Records

Decisions are immutable once accepted. To change a decision, add a new
entry that supersedes it.

---

## ADR-001: Rust compiler, not self-hosted

**Date**: 2026-05-22
**Status**: Accepted

### Context

Hirð needs a compiler. The three viable host languages are Rust, Erlang/Elixir
(compile on BEAM itself), or Hirð itself (self-hosting). Gleam has demonstrated
that a Rust-hosted compiler targeting BEAM is a proven, productive architecture.

### Decision

The Hirð compiler is written in Rust. Self-hosting is a long-term aspiration,
not a near-term milestone.

### Consequences

- A Rust-proficient engineer can be productive on the full stack from day one.
- The Rust ecosystem provides high-quality parsing, diagnostics, and testing
  crates (`chumsky`, `rowan`, `miette`, `insta`).
- The compiler binary is a single native executable — no BEAM required to
  compile (only to run compiled programs).
- No circular dependency between the language and its compiler.

---

## ADR-002: Staged backend — Erlang source first

**Date**: 2026-05-22
**Status**: Accepted

### Context

BEAM languages can target Erlang source, Erlang abstract forms
(`compile:forms/2`), Core Erlang (`cerl` module), or BEAM bytecode directly.
Each level trades debuggability for codegen power.

### Decision

- **v0.1**: emit Erlang source (`.erl` files). Maximum debuggability. Generated
  code is human-inspectable and works with stock `erlc`.
- **v0.2**: emit Erlang abstract forms via `compile:forms/2`. Better for
  incremental compilation and source-span preservation.
- **v0.3+**: may target Core Erlang if the value is clear. The `cerl` API is
  internal and unstable between OTP releases — don't depend on it prematurely.
- **Never**: emit BEAM bytecode directly. The Erlang compiler does that better.

### Consequences

- v0.1 has the fastest path to a working end-to-end demo.
- Generated Erlang is inspectable for debugging.
- Each backend upgrade is an internal change — the language surface is unchanged.
- Core Erlang dependency (if ever adopted) carries maintenance cost per OTP release.

---

## ADR-003: OTP supervision, not a custom runtime

**Date**: 2026-05-22
**Status**: Accepted

### Context

Hirð actors run on BEAM. Supervision could be implemented as a custom runtime
layer or by targeting OTP's existing supervisor behaviors.

### Decision

Use OTP supervision directly. Actors compile to `gen_server` behaviors.
Supervisors compile to `supervisor` behaviors. The Hirð runtime support library
is a thin Erlang wrapper, not a replacement for OTP.

### Consequences

- Hirð programs interoperate with existing OTP applications.
- Battle-tested supervision semantics — decades of production use.
- Constrained by OTP's supervision model (restart strategies, child specs).
- No novel supervision features beyond what OTP provides (v0.1).

---

## ADR-004: DI-style effect handlers in v0.1

**Date**: 2026-05-22
**Status**: Accepted (v0.1 handler-checking scope and lowering strategy refined
by ADR-013)

### Context

Algebraic effect handlers in the Koka/OCaml 5 tradition require CPS
transformation or delimited control — non-trivial compiler infrastructure with
real performance implications. BEAM does not have native support for delimited
continuations.

### Decision

v0.1 effect handlers are dependency-injection-style: a `handle` block provides
function implementations for declared effects, and the compiler routes calls
through them. No resumable continuations, no CPS transformation.

Koka-style handlers are deferred to v0.2+ and may not be needed if DI-style
proves sufficient for the agent-system use case.

### Consequences

- Simpler compiler: no CPS pass, no continuation capture.
- Handlers can mock, dry-run, redirect, and audit — sufficient for v0.1 use cases.
- Handlers cannot resume computations or interleave effects.
- Surface syntax for `handle` blocks must be conservative enough to lower to
  ordinary Erlang function dispatch.

---

## ADR-005: Per-process effect semantics

**Date**: 2026-05-22
**Status**: Accepted

### Context

Effect rows could be transitive (a function's effects include what its callees'
message recipients do) or local (a function's effects describe what the current
process does directly).

### Decision

Effects are per-process and local. A function's effect row describes what the
current process does. Sending a message has a `Send<Msg>` effect. The receiving
actor has its own independent effect summary. The sender's effect row does NOT
transitively include the receiver's effects.

Transitive effect closure is a tooling query (MCP server, effect-graph
analysis), not a type-system feature.

### Consequences

- Type system stays tractable on a runtime where recipients can outlive senders,
  be restarted by supervisors, run on different nodes, and process messages
  asynchronously.
- Effect rows are useful locally (the function's own side effects are visible).
- Whole-system effect analysis requires tooling, not just type checking.

---

## ADR-006: Opaque-capability discipline for stateful resources

**Date**: 2026-05-22
**Status**: Accepted

### Context

BEAM has shared-state escape hatches (ETS, process dictionary,
`persistent_term`). Treating these as ambient effects (`{Mut}`, `{Global}`)
makes too much code "side-effecting" in a way too coarse to be meaningful.

### Decision

Stateful resources are opaque capabilities, not ambient effects. Each resource
has a typed handle with associated permissions, and operations are typed against
the specific capability:

```
type Table<K, V, Perm>
effect EtsRead<Table<K, V>>
fn lookup(t: Table<K, V, Read>, key: K) -> Option<V> ! {EtsRead<t>}
```

The capability must be passed in. The effect references the specific instance.
The same pattern applies to: `Db<Schema>`, `Http<Client>`, `Tool<Name>`,
`Clock`, `Random`, `Log`.

### Consequences

- No ambient `now()`, `random()`, or `log()` — every source of non-determinism
  requires a capability the caller provides.
- Audit graphs show exactly which resources a function touches.
- More function parameters (capabilities must be threaded).
- Standard library design is constrained by this discipline.

---

## ADR-007: Unicode canonicalization at the lexer

**Date**: 2026-05-22
**Status**: Accepted

### Context

A sibling project normalizes ASCII operator sequences to Unicode canonical forms at
save time. This produces one form per operator across the codebase, eliminating
ambiguity for both human and LLM readers.

### Decision

The Hirð lexer normalizes at lex time: `->` normalizes to `→`, `=>` to `⇒`,
`\` to `λ`. Both forms produce identical token streams. The canonical form is
the Unicode version. This is a save-time normalization inherited from
a sibling project.

### Consequences

- One canonical form per operator — unambiguous in generated and analyzed code.
- LLMs see consistent syntax regardless of how the code was authored.
- Developers must configure their editors for Unicode input or rely on
  auto-formatting.
- The lexer must handle both forms transparently.

---

## ADR-008: MSRV 1.92, edition 2024

**Date**: 2026-05-22
**Status**: Accepted

### Context

The workspace was scaffolded with Rust edition 2024 and MSRV 1.92. CI tests
against stable 1.93.

### Decision

Maintain these versions. MSRV bumps require updating `Cargo.toml`, CI, and
README files in lockstep.

### Consequences

- Access to edition 2024 features (e.g., `use<>` in return-position `impl Trait`).
- MSRV lag is minimal (one version behind stable).
- Contributors need Rust 1.92+.

---

## ADR-009: Expression bodies are bare; no block expressions in v0.1

**Date**: 2026-05-31
**Status**: Accepted

### Context

Many constructs have a body: `fn`, `let … in`, `if … then … else`,
`match` arms, and `handle … in`. The surface syntax could brace-delimit
those bodies (a block form) or treat each as a single bare expression.

Brace-delimited bodies in expression position collide with record
literals (`{ name: expr }`), forcing lookahead to tell a block from a
record. That cost only buys something if the language has block
expressions (statement sequences), which v0.1 does not.

### Decision

v0.1 is expression-oriented. Every expression body is a single bare
expression introduced by the construct's own keyword or symbol:
`fn … = e`, `let … in e`, `if … then e else e`, `match … → e`, and
`handle … in e`. There are no block expressions.

Braces `{ }` are reserved for non-expression positions: effect rows
(`! { … }`), record literals and record types, and the member lists of
`handle` and of declaration forms (`actor`, `supervisor`). They never
wrap an expression body, so a `{` where an expression is expected is
unambiguously a record literal. Sequencing within a body uses nested
`let … in`, not statement blocks.

### Consequences

- One uniform rule for bodies; no per-construct bracing exceptions.
- No block-vs-record lookahead is needed in the parser.
- The actor/supervisor handler-body grammar (not yet implemented) follows
  the same bare-body rule.
- If block expressions are ever wanted, they are introduced uniformly
  across all body positions in a single change that supersedes this ADR.

---

## ADR-010: Module and visibility system

**Date**: 2026-06-18
**Status**: Accepted

### Context

Hirð needs a module system: a unit of namespacing, a visibility boundary, and
the mechanism behind the opaque-capability discipline (ADR-006). The design
space spans first-class modules and functors (ML), path-based modules with
`use` imports (Rust, Gleam), and header/implementation splits (C). The sibling
a sibling project project already commits to `use`/export conventions, and parity
keeps the two languages legible to the same readers and tools.

### Decision

The module system is path-based and intentionally small. No first-class
modules, no functors, no module-level abstraction beyond opaque types.

- **One module per file.** A module's name is derived from its file path and is
  authoritative; a file's `module` declaration, if present, must match it.
- **Imports** take three forms, with `.` as the separator:
  `use Mod` (binds the trailing segment as a qualifier),
  `use Mod as M` (binds `M` as the qualifier), and
  `use Mod.{a, b}` (binds `a` and `b` unqualified). Whole-module and aliased
  imports are for qualified access; only selective imports bind names
  unqualified.
- **Qualified access** is `Mod.member`. It is disambiguated from record field
  access (`point.x`) by the receiver: a bare `PascalCase` name that resolves in
  the module namespace is a qualifier; otherwise it is field access on a value.
  The naming convention (ADR naming rules) guarantees the two never overlap.
- **Three type-visibility levels:** `type T` is private (module-only),
  `pub type T` is transparent (name and constructors exported), and
  `pub opaque type T` exports the name but keeps constructors module-private.
  Functions export with `pub fn`. Unprefixed declarations are private.
- **Separate namespaces for types and values**, so `type Email = Email(String)`
  (a type and a like-named constructor) is legal. Duplicate detection runs per
  namespace.
- **Circular imports are detected and rejected** by condensing the import graph
  into strongly connected components.

Opaque types are the mechanism behind capability types (ADR-006): a
capability's constructor is private to its declaring module, so no other code
can mint or forge one. Construction and destructuring of an opaque type outside
its module are compile errors.

Standard-library resolution is deferred: for v0.1, imports resolve only to
modules supplied to the checker, behind a seam that a library search path can
later fill.

### Consequences

- The module system is easy to teach and to read: three import forms, one
  visibility keyword plus an `opaque` modifier, file-based module identity.
- Opaque capabilities (ADR-006) need no separate mechanism; they are ordinary
  opaque types.
- No qualified type paths (`Mod.Type`) in v0.1 — another module's type is named
  by importing it selectively. The casing convention makes the qualified-name
  vs field-access split unambiguous without lookahead.
- Whole-program checking must order modules by dependency and reject cycles,
  rather than checking files independently.

---

## ADR-011: Effect-row representation and crate boundaries

**Date**: 2026-06-24
**Status**: Accepted (§1 crate-placement clause superseded in part by ADR-012
and the remainder by ADR-014, which removes `hird-effects`; the representation,
row union-find, and set-semantics decisions stand)

### Context

Phase 5 adds effect rows to the type system. Function types must carry an
effect row, and the substitution table must allocate and solve row variables
for row polymorphism. The Phase-5 tickets nominally place `EffectRow` and
`Effect` in `hird-effects`, but `hird-effects` depends on `hird-types`, while
`TyFn` (in `hird-types`) must embed an effect row and `Subst` (in `hird-types`)
must manage row variables — a dependency cycle. Row unification is also mutually
recursive with type unification, because parametric effects carry types. Two
further questions follow: how row variables live in the union-find, and how an
effect row is represented so that unification stays sound.

### Decision

1. **The representation lives in `hird-types`.** `Effect`, `EffectRow`, row
   variables, and row unification are defined in `hird-types` alongside `Type`,
   `Subst`, and `unify`; `TyFn` carries an `EffectRow`. `hird-effects` is the
   home for *effect inference* and *handler lowering*, built on top of
   `hird-types` — not for the data types. `hird-ir` uses the `hird-types`
   representation in place of its placeholder.

2. **Row variables use a separate union-find within `Subst`.** Row variables
   are a distinct kind from type variables: they are allocated from a separate
   row-slot table, indexed by a distinct `RowVar` newtype, and share the single
   binding-`level` counter with type variables. Cross-kind binding (solving a
   type variable to a row, or vice versa) is unrepresentable — the kind
   separation is enforced by the Rust type system, not by runtime assertions.
   Generalisation and instantiation quantify and refresh row variables as well
   as type variables; quantified variables record their kind.

   *Rejected*: a single union-find over a `Type`-or-`Row` term with kind-tagged
   slots. It is more compact and avoids duplicated union-find code, but trades
   compile-time kind safety for runtime assertions and perturbs the tested
   type-slot core. The duplicated machinery is small and bounded, and the shared
   level counter means there is no dual-counter coherence cost.

3. **An effect row is an idempotent set with an optional tail, keyed by effect
   head.** `EffectRow` is a single struct: a collection of effects keyed by
   effect-constructor name (`BTreeMap<Name, Vec<Effect>>`, so several effects
   may share one head — `Tool<ReadRepo>` and `Tool<CreateTicket>` coexist) plus
   an `Option<RowVar>` tail. The empty/closed/open distinction is encoded by the
   tail (`None` closed, `Some` open, empty map + `None` the empty row), not by
   separate variants. Rows are idempotent sets (`{Log, Log} = {Log}`).

   The outer key is the effect-constructor name, which is stable under
   substitution, so ordering never depends on unsolved type-variable identities
   (a structural `Ord` over effects would corrupt as variables solve). Row
   unification matches effects by head and unifies the type arguments of
   same-head effects through the ordinary type `unify`; the open/open case
   splits the residual into a fresh tail row variable. Effect equality and
   de-duplication compare *resolved* arguments, never raw variable ids.

4. **Set semantics are a v0.1 commitment.** Idempotent set semantics match
   ADR-004 (DI-style handlers; Koka-style resumable handlers deferred to v0.2+).
   Scoped or duplicated labels and effect ordering — which Koka-style handlers
   may require — are out of scope; if ever adopted they supersede this decision
   with a multiset/ordered representation.

### Consequences

- The foundational type crate owns the effect-row representation; effects build
  on types with no dependency cycle.
- Kind confusion between type and row variables is a compile error.
- Binding soundness obligation: level-lowering and the occurs-check must cross
  from type-space into row-space — through every `TyFn` row and through the type
  arguments of parametric effects — or generalisation over-quantifies a row
  variable and an effect can escape its handler.
- Row unification must be shown to terminate (a decreasing measure on the
  residual rows, plus an occurs-check on row tails), not merely be "idempotent".
- Effect-graph tooling gets a head-keyed index for free.

---

## ADR-012: Effect-inference placement and capability-effect representation

**Date**: 2026-06-29
**Status**: Accepted (supersedes the crate-placement clause of ADR-011 §1;
refines the capability-effect linkage of ADR-006; its `hird-effects` placement
superseded by ADR-014)

### Context

Phase 5's effect-inference work — infer effect rows for function bodies, check
them against declared annotations, and link effects to capabilities — surfaced
two questions that earlier decisions had over-committed or left open.

First, ADR-011 §1 placed effect *inference* in `hird-effects`. But the
inference machinery — the substitution table (with its row variables), the type
environment, the constructor/effect registry, instantiation and row resolution,
expression inference, and the per-node type side-table — is all private to
`hird-check`, and effect inference is inseparable from type inference: an
application's effects are read off the callee's *resolved* function-type row.
Honouring the original placement would force `hird-effects` to depend on
`hird-check` and either re-walk a fully typed tree or expose almost all of the
checker's internals.

Second, ADR-006 and the Phase-5 task call for capability effects that reference
"the specific capability value" (`EtsRead<t>` for a parameter `t`), so an audit
graph can show exactly which resources a function touches. But the effect
representation carries *types*, not values, and the type representation has no
value or singleton form: two parameters of the same type are structurally
identical. Distinguishing them inside the type layer would need per-binding
singleton identities — which fight generalisation, because a non-generalisable
identity that still flows through instantiation and row-argument unification is
either unsound under polymorphism (two call sites collapse to one resource node)
or is a value-substitution dimension in disguise — or it would need to extend
the row with a value dimension, perturbing the just-stabilised idempotent-set
row unification of ADR-011. A static type system cannot, even in principle, name
the runtime value a capability binds to; the most it can know statically is the
binding site.

### Decision

1. **Effect inference lives in `hird-check`, interleaved with type inference.**
   An effect accumulator is threaded through the body walk: an application unions
   its callee's resolved effect row into the enclosing function's row;
   `let`, sequencing, and `match` union their parts; a lambda's body effects
   attach to the lambda's *function-type* row, not the enclosing row (the
   accumulator resets at each lambda boundary). `hird-effects` remains the home
   for handler lowering and any later pure effect-algebra helpers; it does not
   host body inference. This supersedes only the crate-placement clause of
   ADR-011 §1; the representation, row union-find, and set-semantics decisions of
   ADR-011 stand.

2. **Capability effects are represented at the type level; binding-site identity
   is carried as provenance, outside the type system.** `EtsRead<t>` elaborates
   with the capability parameter's *type* as the effect argument
   (`EtsRead<Table<UserId, User, Read>>`); call sites instantiate it through
   ordinary type unification, with no new machinery. Which capability *binding*
   introduced an effect, and the source span of the introducing call, are
   recorded in a provenance side-table during inference, separate from the
   effect row. The type system proves *what kind* of resource is touched; the
   provenance map records *from where*. Audit-graph tooling renders resource
   edges from provenance, not from row identity.

   Effects of the same head and same resolved type arguments are one element of
   the idempotent-set row (ADR-011), so two capabilities of the *same* type are
   not distinguished *within the row*. This is faithful for v0.1: the planner's
   capabilities are distinctly typed, so no same-typed collision arises. True
   per-value distinctness — singleton capability identities, or value-indexed
   effect arguments — is deferred to v0.2+. It is additive: it only refines an
   effect argument from the capability's type to a finer per-binding identity,
   forcing no row-representation change.

   This refines ADR-006's "the effect references the specific instance": for
   v0.1 the static guarantee is the capability's *type* plus binding-site
   provenance, not runtime-value identity (which is dynamic and not statically
   knowable).

### Consequences

- Effect inference reuses the checker's substitution, environment, and the row
  generalisation/occurs-check/level-lowering already established; no cross-crate
  exposure of checker internals, and no second tree walk.
- `hird-effects` is thinner than ADR-011 envisioned — handler lowering and
  helpers only. Acceptable: the row representation and unification it would have
  shared already live in `hird-types`.
- The provenance side-table is shared infrastructure: one map drives both the
  annotation-mismatch diagnostic (offending effect, span at the introducing
  call) and capability-to-resource linkage. Capability linkage is therefore not
  extra scope — the diagnostic needs the map regardless.
- The audit graph is precise on resource *kind* and on *binding site*, but
  conflates two same-typed capabilities bound to different runtime values. A
  documented limitation; no v0.1 demo exercises it.
- The upgrade path to value-precise effects stays open and additive; the type
  layer is never contaminated with value identity, so adopting singletons later
  supersedes only this clause.

---

## ADR-013: v0.1 effect-handler checking scope and lowering

**Date**: 2026-06-30
**Status**: Accepted (refines ADR-004; `hird-effects` lowering placement
superseded by ADR-014)

### Context

ADR-004 commits v0.1 to DI-style `handle` blocks. Implementing them surfaces two
questions, each gated by machinery that does not exist yet.

First, a handle arm `Effect → impl` would ideally be type-checked by matching
`impl` against the effect's operation signature — `Tool<ReadRepo> → mock_read`
should verify `mock_read` accepts `{ path: Path }` and returns `RepoState`. But
v0.1 effects are bare labels: an `effect` declaration records only a name and a
type-parameter arity. The operation signature a handler would be checked against
is introduced by `tool` declarations (Phase 6), which depend on this phase. So
there is nothing to check a handler signature against yet.

Second, a handler must eventually reroute effectful calls in generated Erlang —
by threading handler implementations as parameters, or by process-dictionary
lookup. But there is no Erlang backend yet: `hird-codegen` is a stub and ADR-002
stages source emission as later work, so no lowering strategy can be exercised
end to end.

### Decision

1. **v0.1 handler checking is structural, not signature-directed.** A handle arm
   type-checks iff its head names a declared effect applied at the correct arity
   and its handler expression has a function type. Validating handler argument
   and result types against the effect's operation signature is deferred until
   `tool` declarations introduce those signatures. Unknown effect, wrong effect
   arity, and a non-function handler are the handler-shape errors v0.1 reports.

2. **v0.1 handlers lower to IR only; the chosen Erlang strategy is parameter
   threading.** A handle block lowers to a dedicated IR node carrying the handler
   bindings and the handled body; no Erlang is emitted in this phase. When the
   backend is built, a handler lowers by threading its implementation as an
   explicit parameter through the handled scope, and an effectful call resolves
   to the threaded handler.

   *Rejected*: storing handlers in the process dictionary and looking them up at
   the call site. It needs less plumbing and no arity growth, but introduces
   per-process hidden mutable state, contradicting ADR-005's explicit per-process
   semantics and the explicit-over-implicit tenet. Parameter threading keeps
   handler routing visible in the IR and in any code generated from it.

### Consequences

- v0.1 accepts a handler whose shape is wrong for the effect in ways only an
  operation signature could catch (e.g. wrong argument types); the structural
  check still rejects unknown effects, wrong arity, and non-function handlers.
  The gap closes when tool-declaration signatures land.
- The handled effect row — body effects minus handled effects plus handler
  effects — is computed in `hird-effects` per ADR-011/012; the structural arm
  check and that row computation are what gate a `handle` block in v0.1.
- The IR gains a handler node now; the backend consumes it later with no IR
  change forced by the chosen strategy.
- Parameter threading grows function arity in generated code — acceptable for
  the explicitness, and invisible until the backend exists.

---

## ADR-014: The handler-row helper lives in `hird-types`; `hird-effects` is removed

**Date**: 2026-06-30
**Status**: Accepted (supersedes the crate-placement clauses of ADR-011 §1 and
ADR-012 §1, and the lowering-placement consequence of ADR-013)

### Context

ADR-011 §1 created a `hird-effects` crate for effect inference and handler
lowering; ADR-012 §1 moved inference into `hird-check`, leaving `hird-effects`
"handler lowering and helpers only". The one piece of that scope that exists in
v0.1 — DI-style `handle` blocks — needs a single pure helper: the handled-row
computation `(body − handled) ∪ handler`. It operates only on `EffectRow` and
`Effect`, whose representation and the rest of the row algebra (`unify_row`,
`resolve_row`) already live in `hird-types`. A crate holding one such function,
justified only by an Erlang backend that does not yet exist (source emission is
staged as later work by ADR-002), is a speculative seam rather than a
load-bearing boundary, and it splits the row algebra across two crates.

### Decision

The handled-row computation lives in `hird-types` as the free function
`handle_row`, beside `EffectRow` and the row unification it complements. The
`hird-effects` crate is removed, along with its (unused) entries in the
workspace and in `hird-cli` and `hird-actors`. When the backend is built,
handler lowering (parameter threading, per ADR-013) belongs with code generation
in `hird-codegen`; whether it earns its own crate is decided then, against real
code rather than a placeholder.

### Consequences

- One fewer crate and one fewer dependency edge; all effect-row algebra lives in
  one place.
- No crate boundary stands around code that does not exist yet; the backend's
  handler lowering finds its home when it is written.
- ADR-011 §1's and ADR-012 §1's "lives in `hird-effects`" clauses, and ADR-013's
  "computed in `hird-effects`" consequence, now read: `hird-types`.

---

## Open Decision Slots

The following decisions are tracked as open tickets and will be documented here
when resolved:

| ID | Topic | Resolves in | Ticket |
|----|-------|-------------|--------|
| OD1 | Crash vs error boundary | Phase 8 | hir-fbze |
| OD2 | LLM call typing | Phase 6 | hir-x6cx |
| OD3 | Audit log fidelity | Phase 6 | hir-yum3 |
| OD4 | Tool effect replay semantics | Phase 6 | hir-v3pv |
| OD5 | Actor protocol typing richness | Phase 7 | hir-b2gn |
| OD8 | Send/reply effect tracking | Phase 7 | hir-actn |
