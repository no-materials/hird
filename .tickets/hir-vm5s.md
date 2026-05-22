---
id: hir-vm5s
status: open
deps: [hir-cn1r]
links: []
created: 2026-05-22T21:32:12Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-2, parser, ast]
---
# Phase 2 — Parser and AST

## Goal

Parse the Hirð token stream into a typed AST with full source spans, error
recovery, and diagnostic infrastructure. Produce a grammar specification
document suitable for inclusion in LLM context.

## v0.1 demo relevance

The parser must handle the complete surface syntax that appears in the v0.1
supervised planner demo: function declarations with effect annotations, actor
declarations with typed mailboxes, supervisor declarations, tool effect
declarations, pattern matching on message types, let bindings, lambdas, type
annotations, and module imports.

## Design context

The parser uses `chumsky` for combinator-based parsing with error recovery.
The AST is projected from a `rowan`-backed concrete syntax tree (CST) that
preserves all source information including whitespace and comments — this is
necessary for tooling-grade IDE support and the Glass IR round-trip property.

The grammar specification lives in `docs/grammar.md` in BNF-ish notation. This
document is a first-class project artifact: it will be included in LLM context
windows via `phrasebook.md`, so it must be precise, complete, and readable.

Error recovery is a hard requirement, not a nice-to-have. The parser must
produce a partial AST plus a list of diagnostics on malformed input, not bail
on first error. This is critical for IDE integration and for LLM agents that
may produce slightly malformed code.

**Surface syntax in this phase**: `let` bindings, lambdas, function declarations
with type annotations, algebraic data types, pattern matching, modules with
`use` imports, extern declarations. Effect annotations, actor declarations,
supervisor declarations, and tool declarations are parsed in this phase but only
as syntax — semantic analysis happens in later phases.

## Out of scope

- Type checking or inference (Phase 3).
- Effect row semantics (Phase 5).
- Actor/supervisor semantic validation (Phases 7-8).
- Incremental parsing or `salsa` integration.

## Acceptance Criteria

- `docs/grammar.md` exists with BNF-ish grammar covering all v0.1 surface syntax.
- `chumsky` parser produces a `rowan`-backed CST for all grammar productions.
- Typed AST projection from CST covers: LetExpr, Lambda, FnDecl, TypeDecl (ADT),
  MatchExpr, ModuleDecl, UseDecl, ExternDecl, EffectDecl (syntax only),
  ActorDecl (syntax only), SupervisorDecl (syntax only), ToolDecl (syntax only).
- Every AST node carries a source span.
- Diagnostic infrastructure using `miette` or `ariadne`: errors and warnings with
  source spans, colored terminal output, and structured diagnostic codes.
- Error recovery: parser produces partial AST + diagnostics list on malformed input.
  Snapshot tests confirm partial parse results for at least 5 distinct error patterns
  (missing delimiter, unexpected token, incomplete expression, malformed type
  annotation, duplicate definition).
- Snapshot tests (insta) cover: minimal complete programs, each syntax construct
  in isolation, nested expressions, pattern matching variants, module imports.
- Property tests: "parse then pretty-print produces canonical form" round-trip
  for well-formed inputs.
- `cargo clippy` and `cargo test` pass for `hird-parse` and `hird-ast`.

