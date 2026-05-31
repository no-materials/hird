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
**Status**: Accepted

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
| OD6 | Module and visibility system | Phase 3 | hir-0s3s |
| OD7 | Handler semantics in v0.1 | Phase 5 | hir-mzhn |
| OD8 | Send/reply effect tracking | Phase 7 | hir-actn |
