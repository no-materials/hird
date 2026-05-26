---
id: hir-of12
status: open
deps: [hir-va37]
links: []
created: 2026-05-22T21:36:57Z
type: task
priority: 1
assignee: nomaterials
parent: hir-vm5s
tags: [phase-2, parser, diagnostics, testing]
---
# Parser error recovery and diagnostics

Implement parser error recovery and comprehensive test suite.

**Error recovery**: the parser must recover from errors and continue
producing a partial AST. Strategies:
- Missing closing delimiter: insert synthetic closer, report error, continue.
- Unexpected token in expression position: skip to next synchronization point
  (semicolon, closing brace, top-level keyword), report error.
- Incomplete declaration: parse what's available, mark declaration as erroneous,
  continue to next declaration.
- Malformed type annotation: fall back to untyped AST node, report error.

The parser should never panic or abort. Every input produces an AST (possibly
with error nodes) plus a diagnostic list.

**Diagnostic quality**: each error has a unique code (E0001, E0002, ...), a
primary message, a source span, and ideally a help suggestion. Plain diagnostic
structs in hird-parse (no_std); miette rendering behind `std` feature flag or
in downstream crates.

**Test suite**:
- Snapshot tests for at least 5 distinct error patterns:
  1. Missing closing parenthesis
  2. Unexpected token mid-expression
  3. Incomplete function declaration
  4. Malformed type annotation
  5. Duplicate top-level definition name
- Snapshot tests for partial parse results (verify the AST contains recoverable
  nodes alongside the errors).
- Property tests: random well-formed programs parse then pretty-print to
  canonical form (round-trip).
- Snapshot tests for complete programs exercising all syntax constructs.

## Acceptance Criteria

- Parser never panics on any input.
- All 5 error patterns have snapshot tests showing error messages with spans.
- Partial AST recovery: at least 2 tests show a multi-declaration file where
  one declaration is malformed but others parse correctly.
- Round-trip property test passes for well-formed programs.
- Diagnostic rendering to terminal looks correct (colored spans, primary message,
  help text where applicable).
- Error codes are unique and documented in a brief error index.

