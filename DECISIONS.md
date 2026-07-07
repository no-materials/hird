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

## ADR-015: Tool declarations — desugaring, invocation records, and the standard-library boundary

**Date**: 2026-07-01
**Status**: Accepted (resolves OD2)

### Context

`tool` declarations are the surface for the tool-effect primitive: an auditable,
structured invocation of an external (often LLM-mediated) operation. A
declaration such as

```
tool ReadRepo : { path: Path } → RepoState
tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn ParseError}
```

must give rise to three things: an effect usable in a row (`Tool<ReadRepo>`), a
callable function (`read_repo`), and a structured invocation record describing
each call (tool name, arguments, result, plus a timestamp and caller identity).
Implementing this surfaced four questions earlier decisions had not settled.

First, `Tool<ReadRepo>` is a parametric effect whose argument is a *type*
(effects carry `Type` arguments — ADR-011). But `ReadRepo` is introduced by a
`tool` declaration, not a `type` declaration, so nothing binds `ReadRepo` in the
type namespace for the effect argument to resolve against.

Second, the invocation record is written as a *named record type*
(`ReadRepoInvocation = { tool, args, result, timestamp, caller }`). The type
system has only structural records (anonymous, keyed by label) and nominal ADTs
(named, but built from positional constructors — ADR-010). It has no named-record
type and no type alias: no existing form is simultaneously named and
record-shaped.

Third, `Exn ParseError` shows a tool declaration may carry its own trailing
effect row beyond the implicit `Tool<…>`, and `LLMCall<t>` shows a tool may be
generic. Neither the parser nor the checker handled these.

Fourth, the standard tools (`llm_call`, `http_get`, `http_post`, `read_file`,
`write_file`, `shell`) and their supporting types must exist, but standard-library
resolution is deferred (ADR-010): there is no prelude and no library search path,
only modules handed to the checker.

### Decision

1. **A `tool` declaration desugars to a marker type, a function, and an effect
   use — over a single built-in `Tool` effect.** Declaring `tool ReadRepo`
   registers a nullary nominal type `ReadRepo` (a marker with no constructors), so
   `Tool<ReadRepo>` resolves through ordinary effect-argument elaboration against
   the one built-in parametric effect `Tool` at arity 1 — not a distinct effect
   per tool. It registers a function (`read_repo`) whose type is
   `(args) → result ! ({Tool<ReadRepo>} ∪ declared_row)`, generalised and bound in
   the value environment exactly as an ADT constructor is. A generic tool binds
   its type parameters in a closed elaboration scope and generalises — the path
   ADT constructors already take — and a tool's optional trailing row is unioned
   into the function's row.

2. **The invocation record is a compiler-derived, named *structural* record held
   in a checker side-table — not a nominal type in the surface namespace.** For
   each tool the checker derives a record type
   `{ tool: String, args: <input>, result: <output>, timestamp: Timestamp,
   caller: CallerId }` and stores it under a generated name (`ReadRepoInvocation`)
   in a side-table on the checked file, beside the effect rows and handled effects
   already kept there. `args` and `result` are projected from the tool signature;
   `tool`, `timestamp`, and `caller` are fixed schema fields the signature cannot
   supply (`timestamp` and `caller` are runtime-injected). The record is
   snapshot-testable by name — satisfying "compiler-generated invocation record
   with correct fields" — without minting a constructor or a new type form.

   *Rejected — a single-constructor ADT wrapping the record*
   (`type ReadRepoInvocation = ReadRepoInvocation({ … })`). It reuses existing ADT
   machinery, but the name is skin-deep: reading a field means unwrapping a
   constructor, the constructor tag leaks a nesting layer into any future
   serialisation, and a generic tool forces a *generic* ADT threading the tool's
   type parameters — machinery, not free reuse.

   *Rejected — adding named record types / type aliases to the language.*
   Genuinely useful, but a language feature in its own right (parser, elaboration,
   unification, IR), too large to seed as a side effect of this work and prone to
   being half-designed under its pressure.

   Because the record is derived and not user-referenced, its representation is
   not load-bearing: when an audit sink defines how records are consumed and the
   JSON wire schema is fixed, the derived record can be promoted to whatever form
   that consumer needs — additively, with no migration of user code. Field order
   is deterministic (records are keyed by label), which audit reproducibility
   needs.

3. **LLM calls are schema-typed (resolves OD2).** The standard LLM tool is
   `llm_call<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn ParseError}`:
   the caller supplies a `Schema<t>`, the result type `t` is tied to it through
   ordinary unification, and a result that does not conform raises
   `Exn ParseError`. Raw-text, opaque-response, and distribution-typed (`Dist<t>`)
   alternatives are rejected for v0.1 — the first two discard the typing story,
   the third is deferred as materially more complex. This is the generic-tool case
   of clause 1; it needs no machinery beyond it.

4. **The tool-declaration mechanism ships now; the specific standard tools live in
   test fixtures until stdlib resolution is unlocked.** The general machinery
   (clauses 1–3) is part of the compiler. The concrete standard tools and their
   supporting types (`Prompt`, `Schema<t>`, `Path`, `Url`, `Headers`,
   `HttpResponse`, `Timestamp`, `CallerId`, `TicketId`, `RepoState`, and the
   `Tool`/`Exn` effects) are declared in `.hird` fixtures fed to the checker by the
   test suite, which asserts they parse and type-check. They are *not* hard-coded
   into the checker as built-ins (that ossifies a prelude the checker can never
   unlearn and smuggles stdlib content past ADR-010) and *not* loaded from an
   implicit prelude module (that reopens the resolution ADR-010 defers). When
   ADR-010 is superseded and a library search path exists, the fixtures graduate
   to a real prelude module unchanged.

### Consequences

- One built-in `Tool` effect, parameterised by a per-tool marker type, keeps the
  effect row's head-keyed index meaningful (`Tool<ReadRepo>` and
  `Tool<CreateTicket>` share the `Tool` bucket — ADR-011) and needs no per-tool
  effect registration.
- The invocation-record representation is deliberately not frozen. The audit-log
  work that consumes records inherits an unconstrained choice of wire schema and
  pins field-ordering and singleton-tag concerns then; this decision fixes only
  the field *shape*.
- `tool` is typed `String` in the record because v0.1 has no singleton/literal
  types; its value is compiler-fixed. A later literal-type feature can narrow it
  without changing the record's other fields.
- Handler arms over tool effects can now, in principle, be checked against a
  tool's operation signature (the gap ADR-013 left open). That validation is not
  taken on here; it remains deferred so this work stays declarations-plus-record.
- Standard tools proven by fixtures are exercised end to end by the checker but
  are not importable by user programs until stdlib resolution lands — an accepted,
  documented limitation for v0.1, consistent with ADR-010.

---

## ADR-016: Audit-log wire format and strict-sequential replay

**Date**: 2026-07-02
**Status**: Accepted (resolves OD3 and OD4)

### Context

Tool effects exist to make external invocations auditable (ADR-015 gives
each tool a derived invocation record) and replaceable (ADR-004/013 give
DI-style handlers). Two open decisions remained: what guarantees the audit
log provides (OD3), and whether replay re-executes tools or returns logged
values (OD4). Both had to be resolved without a backend — `hird-codegen`
is a stub and ADR-002 stages Erlang emission as later work — so "implement
the audit log" cannot mean runtime integration. It can, however, mean a
locked wire format with a reference implementation, because the format is
pure data and the future runtime is only one of its producers.

### Decision

1. **Shape and placement.** The audit log is specified as a wire format
   with a Rust reference implementation: a `wire` module in `hird-check`,
   beside the derived invocation records — no new crate (the ADR-014
   lesson; extraction waits for the Erlang emitter as a second consumer)
   and no interpreter (which would contradict ADR-002's staging). The
   implementation's outputs are snapshotted as language-agnostic golden
   files in a versioned `conformance/` directory; the Erlang runtime must
   later reproduce them byte-exactly. The Rust implementation is the
   conformance oracle, not a rival source of truth. The normative format
   specification lives in `docs/tool-effects.md`.

2. **Wire format (resolves OD3).** JSON lines, one record per invocation,
   with envelope fields in fixed order: `schema_version` (required, `1`),
   `tool`, `args`, `result`, `timestamp`, `caller`, and an optional
   observer-populated `meta` object. The writer is canonical and
   deterministic — hand-rolled, `no_std`-compatible, no `serde_json`; no
   whitespace; records in sorted label order; integers exact within `i64`;
   floats shortest-round-trip in plain notation with NaN/infinities not
   wire-representable; ADT values as `{"ctor":…,"args":…}` (`Bool`
   uniformly included); unit as `null`; lists and tuples as arrays.
   Encoding is injective per type, and decoding is type-directed against
   the tool's signature, validating shape, labels, constructors, and
   arities (round-trip property tested).

   Three consequences of the format are locked with it:

   - **`result` is tagged** `{"ok":…}`/`{"err":…}` — failed invocations
     are first-class and replayable.
   - **`duration_ms` is not a record field.** The compiler-derived record
     keeps ADR-015's five fields (`tool`, `args`, `result`, `timestamp`,
     `caller`); transport metadata lives in the optional `meta` envelope
     field, populated by the observer.
   - **Wire-representability is checker-enforced**: function types and
     opaque capability types are rejected in tool signatures (walking
     through nested ADT constructor fields), so every declarable tool's
     records are encodable and a decoded log can never mint a capability.

   Timestamps are RFC 3339 UTC at millisecond precision; timestamps and
   caller ids are injected, never read from an ambient clock. The caller
   id is `"Module.function"` in v0.1; an actor form
   (`"Planner.handle_msg/PlanRepo"`) is a documented provisional extension
   absorbed via a `schema_version` bump when Phase 7 needs it. No
   tamper-proofing in v0.1 — content addressing, chaining, and signatures
   are the upgrade path `schema_version` exists to admit.

   The audit sink is itself a capability: `AuditSink` is passed in, not
   ambient, and audit emission is a handler wrapping the tool effect,
   visible in the effect row. A fixture omitting the sink parameter fails
   to type-check. The default sink writes canonical JSON lines.

3. **Replay (resolves OD4).** Replay returns logged values; re-execution
   is the same program under a live handler. The choice is a handler
   decision, not a language mode. The core is a pure function
   `(log, position, tool, args) → Result<result, Divergence>` with
   **strict sequential** matching: the record at the position must match
   tool and args exactly, and any mismatch — exhausted log, tool
   mismatch, args mismatch — is a hard error carrying a structured
   `Divergence` value. Keyed matching and live fall-through are rejected:
   both reintroduce the nondeterminism replay exists to remove. The log's
   full args and tagged results make failures replay as faithfully as
   successes. Only divergence-reporting ergonomics remain provisional
   pending real runs.

### Consequences

- The wire format is locked and conformance-tested before any runtime
  exists; the Erlang backend inherits a byte-exact contract instead of
  defining one ad hoc.
- Determinism everywhere: canonical bytes make logs diffable, golden
  files stable, and replay divergence detectable by equality.
- The checker gains a new error (wire-representability), closing the gap
  between "declarable tool" and "auditable tool" at compile time.
- Cross-language float formatting is the riskiest byte-exactness
  obligation; the conformance files pin the expected bytes so any
  divergence surfaces as a failing golden test, not a silent drift.
- `meta` is unvalidated, self-describing JSON by design; nothing
  compiler-derived may ever move into it without a schema bump.

---

## ADR-017: Signature-directed handler checking for tool effects

**Date**: 2026-07-06
**Status**: Accepted (closes the handler-signature gap ADR-013 §1 deferred,
using the operation signatures ADR-015 introduced)

### Context

ADR-013 made v0.1 handle-arm checking structural because effects were bare
labels with no operation signature to check a handler against, and noted the
gap would close when tool declarations introduced those signatures. ADR-015
landed tool declarations, so a `Tool<Marker>` arm now has a signature —
`{args} → result`, optionally generic, optionally with a trailing row — and
the deferral's premise is gone for tool effects. Two sub-questions had to be
settled: how to check a handler against a *generic* tool without polymorphic
subsumption machinery, and whether the handler must reproduce the tool's
declared trailing row.

### Decision

A handle arm over `Tool<Marker>` is checked against the tool's operation
signature; non-tool effects keep ADR-013's structural checking unchanged.

- **Lookup is a checker side-table keyed by marker name**, populated at tool
  declaration beside the derived invocation records — not a value-environment
  lookup of the generated function, which user code could shadow.
- **Generic tools check by instantiate-and-unify**: the tool's generalised
  signature is instantiated with fresh type variables and unified with the
  handler's type. This accepts a *monomorphic* handler for a generic tool
  (an `LLMCall` handler fixed at `Schema<Int>`) — an accepted v0.1 gap;
  requiring a handler at least as polymorphic as the tool needs
  skolemisation/polymorphic subsumption the checker does not have.
- **Rows are not part of the match**: the handler unifies against
  `(args) → result` with a fresh open effect row, so a mock may be pure and
  need not carry the tool's declared trailing row (e.g. `Exn<ParseError>`);
  handler effects join the block's row as before.
- **`Tool<X>` where `X` is not a declared tool is an error** ("not a declared
  tool"), not a silent fall-through to the structural check.
- **The structural checks are retained and precede the signature check**:
  unknown effect, wrong effect arity, and a non-function handler (C0031)
  report as before; a signature mismatch has its own code (C0034), so a
  shape error never surfaces as a confusing unification error.

### Consequences

- The mock-doesn't-match-the-tool bug class ADR-013 accepted is now a compile
  error; the v0.1 demo's DI-style mocks are checked against the real tool
  signatures.
- A generic tool can be handled monomorphically without complaint — two arms
  fixing the same tool at different instantiations are each checked
  independently against fresh instantiations. Documented, revisited only if
  polymorphic subsumption ever lands.
- Handling a user-declared `Tool`-headed effect whose argument is not a tool
  marker is no longer expressible; `Tool` is effectively reserved for the
  tool-declaration mechanism in handle arms.

---

## ADR-018: Sum-type mailboxes; actors as a namespace, not values

**Date**: 2026-07-07
**Status**: Accepted (resolves OD5)

### Context

Phase 7 makes actors first-class declarations, and OD5 asked how rich the
actor type system should be in v0.1: plain sum-type mailboxes, session types
(typed state machines over legal message sequences), protocol typing between
actors, or behavioral request/response typing. Implementing actor
declarations also had to settle what an actor's members are to the rest of
the program: whether the message type, state type, and actor name enter the
ordinary namespaces, and what "state encapsulation" means mechanically.

### Decision

1. **Sum-type mailboxes only (resolves OD5).** An actor's mailbox is typed by
   a sum type of message constructors; the compiler checks handler coverage
   and send/request types against it. Session types, protocol typing, and
   behavioral types are future work. The declaration syntax leaves room for
   protocol annotations (the member list is extensible and the trailing
   effect summary already annotates the whole actor), so adding them later is
   an additive change that supersedes this decision.

2. **The message type is an ordinary ADT; the actor is its own namespace.**
   `message: Msg = A | B` registers `Msg` in the type namespace and its
   constructors in the value namespace — senders must be able to construct
   messages. The actor's *name* lives in a third, actor namespace: it is not
   a value (no first-class actors, consistent with ADR-010's
   no-first-class-modules stance), and `spawn(Actor, args…)` is a keyword
   form resolving its first argument there. `Pid<t>` and `ReplyTo<t>` are
   built-in type constructors (the `List`/`Option` precedent); `ReplyTo<t>`
   is distinct from `Pid<t>`, its runtime representation a codegen decision.

3. **State encapsulation is structural.** The state *type* is an ordinary
   type; the state *value* is unreachable from outside because no expression
   form produces it: `spawn` returns `Pid<Msg>`, and the only binders of the
   state value are the handlers' trailing state patterns and the init body's
   result. Referencing the actor name as a value (including `Actor.member`)
   is a dedicated compile error rather than an unbound-name fallback.

4. **The effect summary is checked for equality, per member and in total.**
   Each handler's and init's body row must equal its declared row (the
   function-body rule), and the actor's trailing summary must equal the union
   of the init and handler rows. `spawn` types as
   `Pid<Msg> ! {Spawn<Msg>}`; init's effects are not the spawner's — they run
   in the spawned process (ADR-005's per-process locality).

### Consequences

- The planner demo's actor needs are covered without any protocol machinery;
  the 80% case ships first.
- Message types compose with everything that works on ADTs today:
  exhaustive `match`, constructor schemes, IR lowering, pretty-printing.
- Ordering constraints between messages (e.g. `Init` before `Work`) are not
  expressible in v0.1; encoding them means encoding legal states in the
  state type by hand.
- Actors are module-local in v0.1: the module system does not export actors,
  so cross-module `spawn` is not yet expressible. Lifted when the module
  interface learns actor entries.
- A future session-type layer slots in as new actor members or annotations
  without disturbing the sum-type mailbox core.

---

## ADR-019: Messaging primitives — send, request, and a distinct reply

**Date**: 2026-07-07
**Status**: Accepted (resolves OD8)

### Context

Phase 7's messaging ticket had three open questions. OD8 asked how send and
reply effects appear in effect rows. The request/reply pattern needed a way
for a handler to answer on a `ReplyTo<T>`: the phrasebook's `GetStatus`
handler carries `{Send<PlannerStatus>}`, but `send` is typed over `Pid<Msg>`
and ADR-018 locked `ReplyTo<t>` as a distinct type, not a `Pid` alias — so
replying was not expressible. And `request` blocks for a reply, which raised
timeout semantics.

### Decision

1. **Send and Await are separate simple effects (resolves OD8).**
   `send(pid, msg)` has effect `{Send<Msg>}`; `request(pid, ctor)` has
   `{Send<Msg>, Await<T>}`. Effects are not parameterized by the recipient —
   Pids are runtime values the type system cannot meaningfully track. There
   is no combined `Request<Msg, T>` effect head: keeping the send and the
   blocking wait distinct preserves their different concurrency implications.
   Per ADR-005, all of these are local, per-process effects; transitive
   closure ("what does the recipient do?") stays a tooling query.

2. **`reply` is a fourth keyword primitive, not an overload of `send`.**
   `reply(reply_to: ReplyTo<T>, value: T) -> () ! {Send<T>}`. A reply channel
   is semantically linear (used exactly once, or the requester hangs or gets
   two answers); a dedicated primitive keeps that upgrade path local — a
   future session-type layer enforces exactly-once as a rule about one
   keyword form, instead of first reconstructing which `send`s are replies.
   It also maps 1:1 onto the runtime, where reply (`gen_server:reply/2`) and
   send (cast) are different operations, so codegen needs no type-directed
   dispatch. `reply` carries plain `Send<T>` — no new effect head.

3. **`ReplyTo<T>` is consumable only by `reply`.** It has no other
   operations. The capability stays narrow, and the future linearity check
   stays purely local.

4. **`request` has a fixed 5000ms timeout in v0.1; timeout is a crash.**
   No surface syntax for configuring it. A timed-out `request` exits the
   caller (OTP `gen_server:call/2` semantics) rather than raising a typed
   error, so the effect row stays `{Send<Msg>, Await<T>}` with no `Exn` —
   crash handling is supervision's job (Phase 8, OD1). If configurability is
   ever needed, the extension point is an optional trailing argument to
   `request`; adding it is additive.

### Consequences

- Four keyword primitives: `spawn`, `send`, `request`, `reply`. Each name
  states intent at the call site; none requires first-class actors or
  first-class channels.
- The phrasebook's handler effect rows remain valid as written — replying
  contributes `Send<T>` exactly as the `GetStatus` example already shows.
- Nothing stops a v0.1 program from dropping a `ReplyTo` or replying twice;
  the failure surfaces at runtime as a request timeout. Exactly-once is
  deferred to the session-type layer reserved by ADR-018.
- Timeouts are not tunable per call site in v0.1; a slow-but-legitimate
  request longer than 5000ms cannot be expressed yet.

---

## ADR-020: Actor-to-Erlang mapping — ReplyTo as From, per-constructor dispatch

**Date**: 2026-07-07
**Status**: Accepted

### Context

Actors compile to gen_server modules (ADR-003), but gen_server emission
needs the expression emitter and erlc validation, both Phase 9 work — so
the mapping is locked now and implemented later. ADR-018 deferred
`ReplyTo<t>`'s runtime representation to codegen. The obstacle is that
gen_server's two delivery mechanisms carry a reply address in different
places: `gen_server:call` attaches a `From` term outside the message
(bound by `handle_call`'s second argument), while `gen_server:cast` has
no envelope at all. A message type whose constructor carries a
`ReplyTo<T>` field must decide where that field lives on the wire — and
whether the same constructor may travel both ways, which happens exactly
when a handler forwards a received reply channel to another actor via
`send`.

### Decision

1. **`ReplyTo<T>` is the gen_server `From` term, erased from the wire.**
   A constructor's `ReplyTo` field does not travel in the payload; the
   handler's `reply_to` binding is `handle_call`'s `From` argument.
   `reply` lowers to `gen_server:reply(From, Value)`.

2. **Dispatch is per constructor.** A constructor with a `ReplyTo` field
   is a call constructor: sent by `request`, lowered to
   `gen_server:call` (fixed 5000ms timeout per ADR-019), received by a
   `handle_call` clause. A constructor without one is a cast
   constructor: sent by `send`, lowered to `gen_server:cast`, received
   by `handle_cast`. Each constructor has exactly one wire shape: a bare
   atom when nullary, a tagged tuple otherwise (the ADT mapping), minus
   any `ReplyTo` field.

3. **`ReplyTo` cannot re-enter a message.** `ReplyTo<t>` may appear only
   as a direct field of a message constructor, at most once per
   constructor. A constructor carrying `ReplyTo` is applicable only as
   the message-builder argument of `request`, and that argument must be
   a bare constructor, not an arbitrary `ReplyTo<t> → Msg` function.
   Together these make forwarding a received reply channel inside
   another message inexpressible, so the two-wire-shapes case never
   arises. Storing a received `reply_to` in actor state stays legal:
   replying later from another handler works because `gen_server:reply`
   is envelope-free.

   *Amended 2026-07-07*: a `ReplyTo` field must also be the
   constructor's **only** field. With the builder restricted to a bare
   constructor reference, a constructor carrying payload alongside
   `ReplyTo` could never be applied anywhere — bare, it does not unify
   with `ReplyTo<t> → Msg`, and ordinary application is forbidden — so
   it is rejected at declaration instead of dying as a unification
   failure at the `request` site. Payload-carrying requests are
   inexpressible in v0.1; admitting them later (partial application,
   or a record payload) is additive.

   *Rejected*: always embedding `From` in the payload (call clauses
   rewrap their envelope `From` into the message before dispatch). It
   admits forwarding, but bakes a wire format that is breaking to walk
   back and generates rewrap shims instead of idiomatic Erlang
   (ADR-002's readability goal). Forbid-then-relax is additive in the
   other direction.

4. **Replies are always explicit; `handle_call` never uses the reply
   tuple.** Every generated `handle_call` clause returns
   `{noreply, State}` and every `reply` emits `gen_server:reply`
   directly. Codegen never proves where — or whether — a handler body
   replies, and state-deferred replies need no special case. A dropped
   or double reply remains a runtime timeout per ADR-019.

5. **One Erlang module per actor, uniformly prefixed.** Actor `Planner`
   emits module `hird_planner` (PascalCase → snake_case under a fixed
   `hird_` prefix). The blanket prefix sidesteps collisions with
   OTP/stdlib module names and Erlang reserved words instead of
   detecting them case by case. Constructor atoms are the snake_cased
   constructor names. Function-level naming inside the module is
   emitter detail, not locked here.

6. **Out of scope: handler threading across `spawn`.** How a spawner's
   DI-style handler bindings (ADR-013's parameter threading) reach the
   spawned process is a Phase 9 decision, made alongside the runtime
   support library.

### Consequences

- `request(pid, GetStatus)` lowers to `gen_server:call(Pid, get_status)`;
  the phrasebook's actor block maps onto a gen_server with no surface
  changes.
- Proxy and fan-out patterns — a middleman handing its reply channel to
  a worker that answers the requester directly — are not expressible in
  v0.1. The middleman must `request` the worker itself and relay the
  answer, serializing on its own timeout. Lifting this later means
  admitting an embedded-`From` wire shape for forwarded messages; the
  change is confined to the checker and the emitter.
- With `ReplyTo` as a call constructor's only field, every
  `gen_server:call` payload is a bare constructor atom in v0.1; tagged
  tuples occur only on the cast path.
- The direct-field, at-most-once `ReplyTo` restriction keeps the future
  linearity and session-type checks (reserved by ADR-018/019) local to
  constructor declarations.
- Generated actor modules never collide with existing Erlang code, at
  the cost of a `hird_` prefix on every module name a debugger sees.

---

## Open Decision Slots

The following decisions are tracked as open tickets and will be documented here
when resolved:

| ID | Topic | Resolves in | Ticket |
|----|-------|-------------|--------|
| OD1 | Crash vs error boundary | Phase 8 | hir-fbze |
