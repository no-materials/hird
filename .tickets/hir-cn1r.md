---
id: hir-cn1r
status: open
deps: [hir-b9gf]
links: []
created: 2026-05-22T21:31:53Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-1, lexer]
---
# Phase 1 — Lexer

## Goal

Produce a token stream from Hirð source files. The lexer is responsible for
span tracking, Unicode normalization of canonical operators, and early rejection
of non-canonical naming conventions.

## v0.1 demo relevance

The supervised agent planner demo requires parsing Hirð source. The lexer is
the first stage of the compilation pipeline and must handle all surface syntax
that appears in the demo (actor declarations, effect annotations, tool effect
declarations, pattern matching, function definitions).

## Design context

Unicode normalization is a deliberate inheritance from a sibling project. The lexer
normalizes ASCII operator sequences to their Unicode canonical forms on input:
`->` to `→`, `=>` to `⇒`, `\` to `λ`. This is a save-time normalization — the
canonical form is what gets stored. The rationale is LLM-first consistency: one
form per operator, no ambiguity in generated or analyzed code.

Canonical naming is enforced at the lex layer or immediately post-lex:
`snake_case` for values, `PascalCase` for types, single lowercase letters for
type variables. These rules are compiler-enforced, not
convention.

**Decision point**: whether canonical-naming checks happen at lex time (reject
early, simpler diagnostics) or post-parse (more context for error messages).
Flag in the implementation; lean toward lex-time for simplicity in v0.1.

## Out of scope

- Parsing (Phase 2).
- Effect or actor-specific syntax — the lexer handles tokens generically.
- Incremental lexing or `salsa` integration (deferred).

## Acceptance Criteria

- Token enum covers: keywords (let, fn, actor, supervisor, effect, tool, match,
  use, module, type, handle, spawn, send, request, pub, extern), identifiers,
  integer and float literals, string literals, operators (arithmetic, comparison,
  arrow, fat-arrow, pipe, bang, dot, colon, double-colon), delimiters (parens,
  braces, brackets, comma, semicolon), comments (line and block), EOF.
- Every token carries a source span (byte offset start, byte offset end, source ID).
- Unicode normalization: `->` lexes identically to `→`, `=>` to `⇒`, `\` to `λ`.
  Snapshot tests confirm both forms produce the same token stream.
- Canonical naming: `camelCase` value identifiers produce a diagnostic (error or
  warning — decision to be confirmed).
- Snapshot tests (insta) cover: all keyword tokens, all operator tokens, Unicode
  normalization pairs, string literals with escapes, nested comments, malformed
  input (unterminated strings, invalid Unicode), canonical-naming violations.
- Error recovery: the lexer continues past errors, producing error tokens with
  diagnostics rather than aborting.
- `cargo clippy` and `cargo test` pass for `hird-lex`.

