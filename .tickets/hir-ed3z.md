---
id: hir-ed3z
status: closed
deps: [hir-p78v]
links: []
created: 2026-05-22T21:36:30Z
type: task
priority: 1
assignee: nomaterials
parent: hir-vm5s
tags: [phase-2, parser]
---
# Grammar specification and parser infrastructure

Two deliverables in this ticket:

1. **Grammar specification** (docs/grammar.md): a BNF-ish formal grammar for
   Hirð v0.1 surface syntax. Must cover:
   - Module declarations and use-imports
   - Function declarations with type annotations and effect annotations
   - Let bindings (let ... in ...)
   - Lambda expressions (λ or \)
   - Type declarations (ADTs with constructors)
   - Pattern matching (match ... { ... })
   - Actor declarations (syntax only — semantics in Phase 7)
   - Supervisor declarations (syntax only — semantics in Phase 8)
   - Effect declarations (syntax only — semantics in Phase 5)
   - Tool declarations (syntax only — semantics in Phase 6)
   - Handle blocks
   - Extern declarations
   - Expressions: application, binary operators, if-then-else, literals

   The grammar doc is a first-class artifact for LLM context. It must be precise
   enough to parse unambiguously and readable enough for an LLM to generate
   conforming code from it.

2. **Parser infrastructure**: set up a hand-rolled recursive descent parser with
   cstree-backed CST in hird-parse. `cstree` with `default-features = false`
   (no_std); `hird-parse` stays `#![no_std]`. Define the SyntaxKind enum
   (flat `u16`-repr enum covering both token kinds and node kinds) mapping
   from hird-lex's TokenKind. Set up the CST-to-AST projection in hird-ast.
   Diagnostics: plain structs in hird-parse (no_std); miette rendering behind
   an `std` feature flag or in downstream crates.

   This ticket establishes the parsing framework. The actual grammar productions
   are implemented in the next ticket.

## Acceptance Criteria

- docs/grammar.md exists with complete BNF-ish grammar for v0.1 syntax.
- Grammar covers all constructs listed above.
- Recursive descent parser infrastructure compiles in hird-parse (`#![no_std]`).
- SyntaxKind enum (flat, `u16`-repr) defined with token kinds and node kinds
  for all grammar productions. `From<TokenKind>` conversion implemented.
- cstree GreenNode/SyntaxNode types configured (`default-features = false`).
- Plain diagnostic structs defined in hird-parse (no_std-compatible).
- miette rendering: either behind `std` feature flag or smoke-tested from
  a `#[cfg(feature = "std")]` test / downstream crate.
- One smoke test: parse a trivial expression, verify CST structure.

