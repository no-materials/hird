---
id: hir-0rzf
status: open
deps: [hir-89zs]
links: []
created: 2026-05-22T21:32:56Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-4, ir]
---
# Phase 4 — IR

## Goal

Define a stable, type-annotated intermediate representation that
serves as the contract between the compiler frontend and all downstream
consumers: codegen, LLM tooling, MCP server, IDE plugins, and effect-graph
analysis.

## v0.1 demo relevance

The planner demo's effect-graph JSON output and the MCP server's structured
responses both query the IR. It is the representation that makes "what
does the Planner actor do?" answerable by tooling. Without a clean IR, the
codegen and tooling phases have no stable input.

## Design context

The IR is explicitly typed: every node carries its inferred type and its
effect row. This distinguishes it from the AST, which carries user-written
annotations that may be partial. After type inference and effect inference,
the IR is fully elaborated.

The design inherits a principle from its sibling projects: the IR is an
introspectable form meant for LLM agents and tooling, not just a codegen
input. Design it for queryability first, codegen convenience second.

Key properties:

- **Every node has an explicit type.** No type variables remain after elaboration
  (they are all resolved to concrete types or quantified at let-boundaries).
- **Every function node has an explicit effect row.** Even pure functions have
  the empty effect row `{}`.
- **The IR is serializable.** JSON or a structured binary format, suitable for
  the MCP server to return fragments of.
- **The IR supports round-trip.** AST → IR → pretty-print produces source
  equivalent to the original (modulo whitespace and syntactic sugar desugaring).
  This is a property test, not just a design aspiration.

## Task sequence

1. [ ] [hir-ee8k](hir-ee8k.md) — IR data structures and lowering
2. [ ] [hir-a6lz](hir-a6lz.md) — IR pretty-printer and round-trip tests

## Out of scope

- Effect rows in the IR (those are added when Phase 5 extends the IR).
- Actor and supervisor nodes in the IR (added in Phases 7-8).
- Codegen from IR (Phase 9).
- Incremental IR computation via `salsa` (deferred).

## Acceptance Criteria

- IR data structures defined in `hird-ir` with explicit types on every node.
- IR node kinds: Let, Lambda, App, Match, Constructor, Literal, Var, Module,
  FnDef, TypeDef, ExternRef.
- Lowering pass from typed AST to IR, producing fully elaborated IR.
- Pretty-printer that emits readable Hirð source from IR.
- Round-trip property test: for well-typed programs, AST → infer → IR →
  pretty-print produces source that re-parses and re-infers to an equivalent IR.
- JSON serialization of IR fragments (for MCP server consumption).
- `docs/ir.md` documents the IR node kinds, their fields, and the
  serialization format.
- `cargo clippy` and `cargo test` pass for `hird-ir`.

