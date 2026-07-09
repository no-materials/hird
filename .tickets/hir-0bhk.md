---
id: hir-0bhk
status: open
deps: [hir-xbs5]
links: [hir-fbze, hir-z9rn]
created: 2026-05-22T21:40:54Z
type: task
priority: 1
assignee: nomaterials
parent: hir-cnq8
tags: [phase-8, supervision, errors]
---
# Error-vs-crash boundary and the crash! primitive

Implement the error-vs-crash boundary and the `crash!`/`panic!` primitive as a
frontend feature (lexer → parser → AST → checker → IR). This is Phase 8's
type-system and IR work; Erlang emission is Phase 9.

**Codegen moved to Phase 9**: `crash!` emission folds into hir-zp13 (a new
`IrExpr::Crash` node lowers to an Erlang exit, e.g. `erlang:error/1`); the
supervisor behaviour module is hir-z9rn. This ticket stops at typed IR.

**Error-vs-crash boundary** (resolves OD1):

Domain errors are values in effect rows:
- `Exn ParseError` — a domain error carried as an effect.
- Handled with pattern matching or effect handlers.
- Does NOT kill the process.
- Example: parsing tool output fails → Exn ParseError → caller decides what to do.

Crashes are resource failures that reach the supervisor:
- `crash!("message")` or `panic!("message")` — explicit process termination.
- Runtime failures (out of memory, network disconnect) also crash.
- Propagate as Erlang exits; caught by supervisor for restart.
- Example: network timeout during tool call → crash → supervisor restarts actor.

The language enforces:
- A function with only Exn effects cannot crash (barring bugs/OOM).
- A function that calls crash! has that visible in its signature or call context
  (design: crash! is a divergent function that never returns, like Rust's panic!).
- The compiler can warn if a function catches ALL Exn errors but still might crash
  due to calling a function that uses crash!.

**Documentation**: docs/error-model.md explaining the boundary, when to use each,
and how they interact with effect rows and supervision.

## Acceptance Criteria

- crash!/panic! primitive exists; it is a divergent expression (never returns),
  typed with a fresh result type variable (`∀a. (String) → a`) per ADR-021, so
  it fits any result context.
- crash! lowers to an `IrExpr::Crash` node carrying its message; Erlang emission
  is deferred to hir-zp13.
- Domain errors (Exn effects) stay values in the effect row and do not kill the
  process (type-level distinction).
- docs/error-model.md written with examples and clear guidance.
- OD1 documented in DECISIONS.md.
- Snapshot tests (type/IR level): crash! parsing, crash! typing, crash! IR
  lowering, error-vs-crash distinction in types.
- At least 4 snapshot tests.

