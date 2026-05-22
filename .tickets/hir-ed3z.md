---
id: hir-ed3z
status: open
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

2. **Parser infrastructure**: set up chumsky with rowan-backed CST in hird-parse.
   Define the SyntaxKind enum mapping token kinds to CST node kinds. Set up the
   CST-to-AST projection in hird-ast. Configure miette or ariadne for diagnostic
   rendering.

   This ticket establishes the parsing framework. The actual grammar productions
   are implemented in the next ticket.

## Acceptance Criteria

- docs/grammar.md exists with complete BNF-ish grammar for v0.1 syntax.
- Grammar covers all constructs listed above.
- chumsky parser infrastructure compiles in hird-parse.
- SyntaxKind enum defined with node kinds for all grammar productions.
- rowan GreenNode/SyntaxNode types configured.
- Diagnostic infrastructure (miette or ariadne) integrated: can render an error
  with source span to terminal.
- One smoke test: parse a trivial expression, verify CST structure.

