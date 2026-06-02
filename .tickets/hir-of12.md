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


## Notes

**2026-06-01T12:31:29Z**

Round-trip property AC interpretation (resolves the "pretty-print to canonical form" ambiguity):

The "parse then pretty-print produces canonical form" round-trip is implemented as TWO
properties, NEITHER of which requires a surface pretty-printer:

1. Lossless round-trip: for well-formed inputs, `cst.text() == source` AND the diagnostic
   list is empty. The CST preserves original bytes by design; this is the parser's real
   guarantee and matches the epic's "IR round-trip property" rationale.

2. Canonicalization-equivalence: the ASCII and Unicode spellings of the same program parse
   to identical CST structure (same token-kind sequence), e.g. `\x -> x` vs `λx → x`.

Rationale: canonicalization here is token-KIND unification done in the lexer (`->` and `→`
both lex to TokenKind::Arrow), already covered by hird-lex tests. The CST stays byte-for-byte
lossless on purpose. Textual/layout canonicalization (rewriting `->` to `→`, normalizing
layout) is the ADR-007 save-time formatter concern — OUT OF SCOPE for this ticket and for
hird-parse. No surface pretty-printer is built here. A `hird fmt`-style canonicalizing
formatter, if wanted, is a separate future ticket, distinct from hir-a6lz (which pretty-prints
the lowered IR, not surface syntax).

**2026-06-01T13:54:44Z**

Decision: error-pattern #5 "duplicate top-level definition name" is DEFERRED out of this ticket.

Rationale: duplicate-definition detection is name resolution, and to be a complete feature it
must span cross-module imports/exports (local-vs-local, import-vs-local, import-vs-import
collisions), not just a single file. A single-file-only check would be a half-feature. Name
resolution is owned by the module system, so the requirement moves to hir-i0u7 (see the note
there); collision SEMANTICS are an OD6 design call (hir-0s3s).

Consequence for this ticket: hird-parse stays purely syntactic (no symbol table, no name
checks). The error-recovery suite covers 4 SYNTACTIC patterns: missing closing delimiter,
unexpected token mid-expression, incomplete declaration, malformed type annotation.
Recommendation: add one more purely-syntactic pattern (e.g. missing `in` after `let`, or
missing `=` before a fn body) so recovery coverage stays at 5 distinct shapes without the
deferred semantic check. Update the "5 distinct error patterns" AC accordingly.

**2026-06-01T14:01:37Z**

Error-recovery pattern set FINALIZED (resolves the open recommendation in the deferral note).
The 5 distinct patterns for the recovery snapshot tests are all purely syntactic:
  1. Missing closing delimiter
  2. Unexpected token mid-expression
  3. Incomplete declaration
  4. Malformed type annotation
  5. Missing `=` before a fn body   (e.g. `fn foo() 42`)

Pattern 5 replaces the deferred "duplicate top-level definition name" (now owned by hir-i0u7).
All five are syntactic, so hird-parse needs no symbol table. The "5 distinct error patterns" AC
stays at 5.

**2026-06-01T14:01:37Z**

Decision: miette diagnostic rendering lives behind a `std` feature on hird-parse (option A).
Add a `std` feature that pulls miette and provides ParseDiagnostic -> miette conversion +
rendering; the crate stays #![no_std] by default. Rendering is therefore testable in-crate, so
the "rendering looks correct (colored spans, primary message, help text)" AC is satisfied here
without standing up hird-cli.

Extraction trigger (do NOT build now): when a SECOND diagnostic consumer/renderer appears
(hird-cli, or the type/effect phases emitting their own diagnostics), extract the diagnostic
types + std-gated rendering into a shared `hird-diagnostics` crate. Deferred under "no
speculative abstraction" + incrementalism; this note is the breadcrumb for that extraction.
